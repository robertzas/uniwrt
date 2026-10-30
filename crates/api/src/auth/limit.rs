//! Login throttling (features/security.md: login rate limiting and lockout), in memory:
//!
//! - **Per client:** 10 failed logins within 15 minutes, and the client waits until the oldest
//!   of them ages out. IPv6 clients count per /64, the smallest network a home gets.
//! - **Per username:** after 5 failures in a row, whoever tries waits 1 minute, doubling with
//!   each further failure up to 15 minutes; a success resets it. This slows guessing from many
//!   addresses without locking the owner out for long.
//!
//! The state is bounded ([`MAX_TRACKED`] entries per kind; the least recent go first) and lost
//! at restart, which only lifts the waits early.

use std::{
    collections::{HashMap, VecDeque},
    net::IpAddr,
    sync::{Mutex, PoisonError},
    time::Duration,
};

use tokio::time::Instant;

const CLIENT_WINDOW: Duration = Duration::from_mins(15);
const CLIENT_FAILURES: usize = 10;
const USER_FREE_FAILURES: u32 = 5;
const USER_FIRST_WAIT: Duration = Duration::from_mins(1);
const USER_MAX_WAIT: Duration = Duration::from_mins(15);
pub(crate) const MAX_TRACKED: usize = 10_000;

#[derive(Default)]
pub(crate) struct LoginLimiter {
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    clients: HashMap<IpAddr, VecDeque<Instant>>,
    users: HashMap<String, Streak>,
}

struct Streak {
    failures: u32,
    last: Instant,
}

impl Streak {
    fn until(&self) -> Option<Instant> {
        let over = self.failures.checked_sub(USER_FREE_FAILURES)?;
        let wait = USER_FIRST_WAIT
            .saturating_mul(1 << over.min(10))
            .min(USER_MAX_WAIT);
        Some(self.last + wait)
    }
}

impl LoginLimiter {
    /// How long `ip` must wait before trying `username` again; `None` when it may try now.
    pub(crate) fn wait(&self, ip: IpAddr, username: &str) -> Option<Duration> {
        let now = Instant::now();
        let mut state = self.lock();
        let client = state.clients.get_mut(&key(ip)).and_then(|failures| {
            while failures
                .front()
                .is_some_and(|at| now - *at >= CLIENT_WINDOW)
            {
                failures.pop_front();
            }
            (failures.len() >= CLIENT_FAILURES)
                .then(|| failures.front().map(|oldest| *oldest + CLIENT_WINDOW))
                .flatten()
        });
        let user = state.users.get(username).and_then(Streak::until);
        let until = client.max(user)?;
        (until > now).then(|| until - now)
    }

    pub(crate) fn failed(&self, ip: IpAddr, username: &str) {
        let now = Instant::now();
        let mut state = self.lock();
        if state.clients.len() >= MAX_TRACKED && !state.clients.contains_key(&key(ip)) {
            evict(&mut state.clients, |failures| failures.back().copied());
        }
        let failures = state.clients.entry(key(ip)).or_default();
        failures.push_back(now);
        if failures.len() > CLIENT_FAILURES {
            failures.pop_front();
        }
        if state.users.len() >= MAX_TRACKED && !state.users.contains_key(username) {
            evict(&mut state.users, |streak| Some(streak.last));
        }
        let streak = state.users.entry(username.to_owned()).or_insert(Streak {
            failures: 0,
            last: now,
        });
        streak.failures = streak.failures.saturating_add(1);
        streak.last = now;
    }

    /// A successful login ends the username's streak; the client's failures still count.
    pub(crate) fn succeeded(&self, username: &str) {
        self.lock().users.remove(username);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Clients by address, IPv6 ones by their /64 (IPv4-mapped addresses as IPv4).
fn key(ip: IpAddr) -> IpAddr {
    match ip.to_canonical() {
        IpAddr::V6(ip) => IpAddr::V6((u128::from(ip) & !u128::from(u64::MAX)).into()),
        ip @ IpAddr::V4(_) => ip,
    }
}

/// Removes the entry whose last failure is the oldest.
fn evict<K: Clone + Eq + std::hash::Hash, V>(
    map: &mut HashMap<K, V>,
    last: impl Fn(&V) -> Option<Instant>,
) {
    if let Some(oldest) = map
        .iter()
        .min_by_key(|(_, value)| last(value))
        .map(|(key, _)| key.clone())
    {
        map.remove(&oldest);
    }
}

#[cfg(test)]
mod tests {
    use std::{net::IpAddr, time::Duration};

    use super::{LoginLimiter, MAX_TRACKED};

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    #[tokio::test(start_paused = true)]
    async fn a_username_waits_after_five_failures_in_a_row_doubling_to_fifteen_minutes() {
        let limiter = LoginLimiter::default();
        // Each from its own address, as a distributed guess would come.
        let mut n = 0;
        let mut fail = |limiter: &LoginLimiter| {
            n += 1;
            limiter.failed(ip(&format!("10.0.0.{n}")), "admin");
        };
        for _ in 0..5 {
            assert_eq!(limiter.wait(ip("10.0.1.1"), "admin"), None);
            fail(&limiter);
        }
        assert_eq!(
            limiter.wait(ip("10.0.1.1"), "admin"),
            Some(Duration::from_mins(1))
        );
        assert_eq!(limiter.wait(ip("10.0.1.1"), "other"), None);
        tokio::time::advance(Duration::from_mins(1)).await;
        assert_eq!(limiter.wait(ip("10.0.1.1"), "admin"), None);
        fail(&limiter);
        assert_eq!(
            limiter.wait(ip("10.0.1.1"), "admin"),
            Some(Duration::from_mins(2))
        );
        for _ in 0..5 {
            fail(&limiter);
        }
        assert_eq!(
            limiter.wait(ip("10.0.1.1"), "admin"),
            Some(Duration::from_mins(15))
        );

        limiter.succeeded("admin");
        assert_eq!(limiter.wait(ip("10.0.1.1"), "admin"), None);
    }

    #[tokio::test(start_paused = true)]
    async fn a_client_waits_after_ten_failures_in_fifteen_minutes() {
        let limiter = LoginLimiter::default();
        for i in 0..10 {
            assert_eq!(limiter.wait(ip("192.168.1.20"), "x"), None);
            limiter.failed(ip("192.168.1.20"), &format!("user{i}"));
            tokio::time::advance(Duration::from_mins(1)).await;
        }
        // The first failure ages out 15 minutes after it: 5 minutes from now.
        assert_eq!(
            limiter.wait(ip("192.168.1.20"), "x"),
            Some(Duration::from_mins(5))
        );
        assert_eq!(
            limiter.wait(ip("::ffff:192.168.1.20"), "x"),
            Some(Duration::from_mins(5))
        );
        assert_eq!(limiter.wait(ip("192.168.1.21"), "x"), None);
        // A success does not clear the client's failures.
        limiter.succeeded("user0");
        assert!(limiter.wait(ip("192.168.1.20"), "x").is_some());
        tokio::time::advance(Duration::from_mins(5)).await;
        assert_eq!(limiter.wait(ip("192.168.1.20"), "x"), None);
    }

    #[tokio::test(start_paused = true)]
    async fn ipv6_clients_count_per_slash_64() {
        let limiter = LoginLimiter::default();
        for i in 0..10 {
            limiter.failed(ip(&format!("2001:db8:1:2::{i:x}")), &format!("u{i}"));
        }
        assert!(limiter.wait(ip("2001:db8:1:2:ffff::1"), "x").is_some());
        assert_eq!(limiter.wait(ip("2001:db8:1:3::1"), "x"), None);
    }

    #[tokio::test(start_paused = true)]
    async fn the_state_stays_bounded() {
        let limiter = LoginLimiter::default();
        for i in 0..=MAX_TRACKED {
            let ip = IpAddr::from(u32::try_from(i).unwrap().to_be_bytes());
            limiter.failed(ip, &format!("u{i}"));
            tokio::time::advance(Duration::from_millis(1)).await;
        }
        let state = limiter.lock();
        assert_eq!(state.clients.len(), MAX_TRACKED);
        assert_eq!(state.users.len(), MAX_TRACKED);
        assert!(!state.users.contains_key("u0"), "the oldest went first");
    }
}
