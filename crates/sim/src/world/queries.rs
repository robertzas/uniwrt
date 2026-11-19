//! The gateway's dnsmasq query log (features/network.md, Query statistics; D-133), in the
//! extra form OpenWrt's `logqueries` gives: while the gateway's disk turns it on
//! ([`World::set_query_log`]), each connected client asks now and then for a popular name
//! (answered from the cache or forwarded upstream), a local one (answered from the leases'
//! hosts file) or a name the blocklist the controller pushed blocks (`config … is NXDOMAIN`).
//! Clients whose DNS the firewall redirects to the unfiltered instance ask that instance, a
//! process of its own, which blocks nothing.
//!
//! The log draws from a generator of its own, so turning it on leaves the rest of the world as
//! it would have been.

use std::{collections::BTreeSet, net::Ipv4Addr};

use uniwrt_core::MacAddr;

use super::World;
use crate::rng::Rng;

/// What the gateway's disk says of its query log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QueryLog {
    /// A sample of the names the pushed blocklist blocks; none before one is pushed.
    pub(crate) blocked: Vec<String>,
    /// The clients redirected to the unfiltered instance.
    pub(crate) bypass: BTreeSet<MacAddr>,
    /// The site's local domain (dnsmasq's `domain`).
    pub(crate) domain: String,
}

/// The query log's state in the world.
pub(crate) struct Queries {
    log: Option<QueryLog>,
    /// The next serial of the main and of the unfiltered instance.
    serials: [u64; 2],
    rng: Rng,
}

impl Queries {
    pub(crate) fn new(seed: u64) -> Self {
        Self {
            log: None,
            serials: [1, 1],
            rng: Rng::new(seed ^ 0x0064_6e73),
        }
    }
}

/// The main instance's and the unfiltered instance's pids.
const PIDS: [u32; 2] = [2092, 2161];
/// The chance a connected client asks something in a second.
const RATE: f64 = 0.03;
/// The upstream the gateway forwards to.
const UPSTREAM: &str = "1.1.1.1";
/// daemon.info.
const PRIORITY: u8 = 30;
/// Names clients ask for, the most asked first.
const POPULAR: [&str; 24] = [
    "www.google.com",
    "connectivitycheck.gstatic.com",
    "i.ytimg.com",
    "www.youtube.com",
    "graph.facebook.com",
    "api.apple.com",
    "time.apple.com",
    "outlook.office365.com",
    "www.netflix.com",
    "open.spotify.com",
    "github.com",
    "en.wikipedia.org",
    "cdn.jsdelivr.net",
    "fonts.googleapis.com",
    "www.amazon.com",
    "weather.example.net",
    "updates.example.org",
    "mqtt.example.io",
    "pool.ntp.org",
    "www.reddit.com",
    "discord.com",
    "api.openweathermap.org",
    "firmware.example.com",
    "news.example.com",
];
const QTYPES: [&str; 4] = ["A", "A", "AAAA", "HTTPS"];

impl World {
    /// The gateway's query log as its disk now sets it; `None` while it logs no queries.
    pub(crate) fn set_query_log(&mut self, log: Option<QueryLog>) {
        self.queries.log = log;
    }

    /// This second's queries.
    pub(super) fn step_queries(&mut self) {
        let Some(log) = self.queries.log.take() else {
            return;
        };
        if self.nodes[0].up() {
            let asking: Vec<_> = self
                .clients
                .iter()
                .enumerate()
                .filter(|(_, client)| client.home() && Self::attachment(client).is_some())
                .map(|(index, _)| index)
                .collect();
            for client in asking {
                if self.queries.rng.chance(RATE) {
                    self.query(&log, client);
                }
            }
        }
        self.queries.log = Some(log);
    }

    fn query(&mut self, log: &QueryLog, client: usize) {
        let (ip, mac) = (self.clients[client].ip, self.clients[client].mac);
        let unfiltered = usize::from(log.bypass.contains(&mac));
        let now = self.now_ms();
        let rng = &mut self.queries.rng;
        let qtype = QTYPES[rng.below(QTYPES.len() as u64) as usize];
        let roll = rng.unit();
        let (name, answer) = if roll < 0.12 && unfiltered == 0 && !log.blocked.is_empty() {
            let name = log.blocked[skewed(rng, log.blocked.len())].clone();
            let answer = format!("config {name} is NXDOMAIN");
            (name, answer)
        } else if roll < 0.17 {
            let other = &self.clients[rng.below(self.clients.len() as u64) as usize];
            let host = other.hostname.as_deref().unwrap_or("printer");
            let name = format!("{host}.{}", log.domain);
            let answer = format!("/tmp/hosts/dhcp.cfg01411c {name} is {}", other.ip);
            (name, answer)
        } else {
            let name = POPULAR[skewed(rng, POPULAR.len())].to_owned();
            let answer = if rng.chance(0.55) {
                format!("cached {name} is {}", address(rng))
            } else {
                format!("forwarded {name} to {UPSTREAM}")
            };
            (name, answer)
        };
        let port = 1024 + rng.below(64_000);
        let at = now + rng.below(1000) as i64;
        let serial = self.queries.serials[unfiltered];
        self.queries.serials[unfiltered] += 1;
        let head = format!("dnsmasq[{}]: {serial} {ip}/{port}", PIDS[unfiltered]);
        self.log(
            0,
            at,
            PRIORITY,
            format!("{head} query[{qtype}] {name} from {ip}"),
        );
        self.log(0, at, PRIORITY, format!("{head} {answer}"));
    }
}

/// An index below `len`, the low ones drawn more often (a uniform draw, squared), so some
/// names are asked for much more than others.
fn skewed(rng: &mut Rng, len: usize) -> usize {
    ((rng.unit().powi(2) * len as f64) as usize).min(len - 1)
}

/// A public address an answer names.
fn address(rng: &mut Rng) -> Ipv4Addr {
    Ipv4Addr::new(
        [104, 142, 151, 172][rng.below(4) as usize],
        rng.byte(),
        rng.byte(),
        rng.byte().max(1),
    )
}

#[cfg(test)]
#[path = "queries_tests.rs"]
mod tests;
