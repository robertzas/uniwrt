//! How long each resolution is kept, and which one answers a query.

use std::time::Duration;

use uniwrt_core::Timestamp;
use uniwrt_model::metric::Resolution;

/// How long each resolution is kept (features/observability.md, Metrics).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Retention {
    pub raw: Duration,
    pub minute: Duration,
    pub hour: Duration,
}

impl Default for Retention {
    fn default() -> Self {
        Self {
            raw: Duration::from_hours(48),
            minute: Duration::from_hours(14 * 24),
            hour: Duration::from_hours(400 * 24),
        }
    }
}

/// The longest span read from raw windows (1,440 of them), and from minutes (11,520).
const RAW_SPAN: Duration = Duration::from_hours(6);
const MINUTE_SPAN: Duration = Duration::from_hours(8 * 24);

impl Retention {
    pub const fn of(&self, resolution: Resolution) -> Duration {
        match resolution {
            Resolution::Raw => self.raw,
            Resolution::Minute => self.minute,
            Resolution::Hour => self.hour,
        }
    }

    /// The resolution that answers a query for `from..to` at `now`: the finest one that still
    /// holds `from` and gives a manageable number of points over the span. Missing tails are the
    /// store's to fill from finer ones (rollups lag behind).
    pub fn resolution(&self, from: Timestamp, to: Timestamp, now: Timestamp) -> Resolution {
        let span = to.saturating_duration_since(from);
        let holds = |kept: Duration| from >= now.saturating_sub(kept);
        if span <= RAW_SPAN && holds(self.raw) {
            Resolution::Raw
        } else if span <= MINUTE_SPAN && holds(self.minute) {
            Resolution::Minute
        } else {
            Resolution::Hour
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use uniwrt_core::Timestamp;
    use uniwrt_model::metric::Resolution;

    use super::Retention;

    #[test]
    fn the_finest_resolution_that_holds_the_span_answers() {
        let now = Timestamp::from_unix_secs(1_800_000_000);
        let ago = |hours| now.saturating_sub(Duration::from_hours(hours));
        let retention = Retention::default();
        assert_eq!(retention.resolution(ago(1), now, now), Resolution::Raw);
        assert_eq!(retention.resolution(ago(6), now, now), Resolution::Raw);
        assert_eq!(retention.resolution(ago(24), now, now), Resolution::Minute);
        assert_eq!(
            retention.resolution(ago(7 * 24), now, now),
            Resolution::Minute
        );
        // An hour three days back: raw windows are gone, minutes are kept.
        assert_eq!(
            retention.resolution(ago(72), ago(71), now),
            Resolution::Minute
        );
        assert_eq!(
            retention.resolution(ago(30 * 24), now, now),
            Resolution::Hour
        );
        // A day a month back: only hours are kept.
        assert_eq!(
            retention.resolution(ago(30 * 24), ago(29 * 24), now),
            Resolution::Hour
        );
    }
}
