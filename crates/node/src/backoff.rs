//! Jittered exponential backoff for reconnects (ARCHITECTURE.md, Node protocol: 1 s doubling to
//! 60 s). Each delay is drawn from the upper half of the current step ("equal jitter"), so nodes
//! that lost the controller together don't come back in lockstep.

use std::{
    collections::hash_map::RandomState,
    hash::{BuildHasher as _, Hasher as _},
    time::Duration,
};

#[derive(Debug, Clone)]
pub(crate) struct Backoff {
    min: Duration,
    max: Duration,
    step: u32,
    rng: SplitMix64,
}

impl Backoff {
    pub(crate) fn new(min: Duration, max: Duration, seed: u64) -> Self {
        Self {
            min,
            max: max.max(min),
            step: 0,
            rng: SplitMix64(seed),
        }
    }

    /// The next delay: within [step / 2, step], where the step doubles from `min` up to `max`.
    pub(crate) fn next_delay(&mut self) -> Duration {
        let step = self
            .min
            .saturating_mul(1_u32.checked_shl(self.step).unwrap_or(u32::MAX))
            .min(self.max);
        self.step = self.step.saturating_add(1);
        let ms = u64::try_from(step.as_millis()).unwrap_or(u64::MAX);
        let half = ms / 2;
        Duration::from_millis(ms - half + self.rng.next() % (half + 1))
    }

    /// Starts over from `min`, after a connection that stayed up.
    pub(crate) fn reset(&mut self) {
        self.step = 0;
    }
}

/// A seed that differs between links and between runs; jitter needs no cryptographic quality.
pub(crate) fn random_seed() -> u64 {
    RandomState::new().build_hasher().finish()
}

#[derive(Debug, Clone)]
struct SplitMix64(u64);

impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::Backoff;

    const S: Duration = Duration::from_secs(1);

    #[test]
    fn delays_double_up_to_the_cap_with_jitter_in_the_upper_half() {
        let mut backoff = Backoff::new(S, 60 * S, 42);
        for step in [1, 2, 4, 8, 16, 32, 60, 60, 60] {
            let step = step * S;
            let delay = backoff.next_delay();
            assert!(delay >= step / 2 && delay <= step, "{delay:?} for {step:?}");
        }
    }

    #[test]
    fn reset_starts_over_and_the_seed_makes_it_deterministic() {
        let mut a = Backoff::new(S, 60 * S, 7);
        let mut b = Backoff::new(S, 60 * S, 7);
        let first: Vec<_> = (0..20).map(|_| a.next_delay()).collect();
        assert_eq!(first, (0..20).map(|_| b.next_delay()).collect::<Vec<_>>());
        a.reset();
        assert!(a.next_delay() <= S);
    }

    #[test]
    fn jitter_spreads_a_fleet() {
        let delays: std::collections::BTreeSet<_> = (0..50)
            .map(|seed| Backoff::new(S, 60 * S, seed).next_delay())
            .collect();
        assert!(delays.len() > 25, "{delays:?}");
    }

    #[test]
    fn many_attempts_do_not_overflow() {
        let mut backoff = Backoff::new(S, 60 * S, 1);
        for _ in 0..100 {
            assert!(backoff.next_delay() <= 60 * S);
        }
    }
}
