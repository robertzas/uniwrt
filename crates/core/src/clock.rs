//! The wall clock as an injected dependency, so time-dependent code is deterministic in tests.

use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    },
    time::{Duration, SystemTime},
};

use crate::{Timestamp, time};

/// The source of wall-clock time. Monotonic timing (intervals, timeouts) uses tokio's clock
/// instead, which tests control with `tokio::time::pause`.
pub trait Clock: Send + Sync + fmt::Debug + 'static {
    fn now(&self) -> Timestamp;
}

/// The system's wall clock. Only composition roots (the binary, test harnesses) construct it.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::from_system_time(SystemTime::now()) // arch-check: allow (the one wall clock)
    }
}

/// A clock that moves only when told to. Clones share one time.
#[derive(Debug, Clone)]
pub struct ManualClock(Arc<AtomicI64>);

impl ManualClock {
    pub fn new(start: Timestamp) -> Self {
        Self(Arc::new(AtomicI64::new(start.as_unix_ms())))
    }

    pub fn set(&self, now: Timestamp) {
        self.0.store(now.as_unix_ms(), Ordering::Relaxed);
    }

    pub fn advance(&self, by: Duration) {
        let by = time::saturating_ms(by);
        self.0.update(Ordering::Relaxed, Ordering::Relaxed, |ms| {
            ms.saturating_add(by)
        });
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Timestamp {
        Timestamp::from_unix_ms(self.0.load(Ordering::Relaxed))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{Clock, ManualClock, SystemClock};
    use crate::Timestamp;

    #[test]
    fn manual_clock_moves_only_when_told_and_clones_share_it() {
        let clock = ManualClock::new(Timestamp::from_unix_secs(100));
        let shared = clock.clone();
        assert_eq!(shared.now(), Timestamp::from_unix_secs(100));
        clock.advance(Duration::from_millis(1_500));
        assert_eq!(shared.now(), Timestamp::from_unix_ms(101_500));
        clock.set(Timestamp::UNIX_EPOCH);
        assert_eq!(shared.now(), Timestamp::UNIX_EPOCH);
    }

    #[test]
    fn system_clock_is_after_this_code_was_written() {
        assert!(SystemClock.now() > Timestamp::from_unix_secs(1_792_000_000));
    }
}
