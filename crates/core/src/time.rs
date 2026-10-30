//! Timestamps: Unix milliseconds in UTC, with saturating arithmetic and RFC 3339 display.

use std::{
    fmt,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

/// A point in time as milliseconds since the Unix epoch, UTC. It serializes as that integer
/// (compact in WebSocket deltas and SQLite, and `new Date(ms)` in the UI) and displays as RFC 3339
/// (`2026-10-27T09:30:00Z`, with milliseconds only when they are not zero).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(transparent)]
pub struct Timestamp(i64);

impl Timestamp {
    pub const UNIX_EPOCH: Self = Self(0);

    pub const fn from_unix_ms(ms: i64) -> Self {
        Self(ms)
    }

    pub const fn from_unix_secs(secs: i64) -> Self {
        Self(secs.saturating_mul(1_000))
    }

    /// A system time, saturating at the range of `i64` milliseconds (±292 million years).
    pub fn from_system_time(time: SystemTime) -> Self {
        match time.duration_since(UNIX_EPOCH) {
            Ok(after) => Self(saturating_ms(after)),
            Err(before) => Self(-saturating_ms(before.duration())),
        }
    }

    pub const fn as_unix_ms(self) -> i64 {
        self.0
    }

    #[must_use]
    pub fn saturating_add(self, duration: Duration) -> Self {
        Self(self.0.saturating_add(saturating_ms(duration)))
    }

    #[must_use]
    pub fn saturating_sub(self, duration: Duration) -> Self {
        Self(self.0.saturating_sub(saturating_ms(duration)))
    }

    /// The time from `earlier` to `self`, or zero when `earlier` is later.
    pub fn saturating_duration_since(self, earlier: Self) -> Duration {
        u64::try_from(self.0.saturating_sub(earlier.0))
            .map_or(Duration::ZERO, Duration::from_millis)
    }
}

pub(crate) fn saturating_ms(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (days, ms_of_day) = (self.0.div_euclid(86_400_000), self.0.rem_euclid(86_400_000));
        let (year, month, day) = civil_from_days(days);
        let secs = ms_of_day / 1_000;
        write!(
            f,
            "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
            secs / 3_600,
            secs / 60 % 60,
            secs % 60
        )?;
        match ms_of_day % 1_000 {
            0 => f.write_str("Z"),
            ms => write!(f, ".{ms:03}Z"),
        }
    }
}

impl fmt::Debug for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// Howard Hinnant's `civil_from_days` for days since 1970-01-01 (proleptic Gregorian calendar).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468; // days since 0000-03-01
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
    let month = if month_from_march < 10 {
        month_from_march + 3
    } else {
        month_from_march - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::Timestamp;

    #[test]
    fn displays_reference_instants_as_rfc3339() {
        // Expected values come from `date -u -d @<secs> +%Y-%m-%dT%H:%M:%SZ`.
        for (secs, expected) in [
            (0, "1970-01-01T00:00:00Z"),
            (951_782_400, "2000-02-29T00:00:00Z"),
            (951_868_800, "2000-03-01T00:00:00Z"),
            (1_709_251_199, "2024-02-29T23:59:59Z"),
            (1_792_622_700, "2026-10-21T22:45:00Z"),
            (4_102_444_799, "2099-12-31T23:59:59Z"),
            (-1, "1969-12-31T23:59:59Z"),
            (-86_400 * 365, "1969-01-01T00:00:00Z"),
        ] {
            assert_eq!(
                Timestamp::from_unix_secs(secs).to_string(),
                expected,
                "{secs}"
            );
        }
        assert_eq!(
            Timestamp::from_unix_ms(1_792_622_700_042).to_string(),
            "2026-10-21T22:45:00.042Z"
        );
        assert_eq!(
            Timestamp::from_unix_ms(-1).to_string(),
            "1969-12-31T23:59:59.999Z"
        );
    }

    #[test]
    fn converts_system_time() {
        let at = |ms| UNIX_EPOCH + Duration::from_millis(ms);
        assert_eq!(Timestamp::from_system_time(at(1_500)).as_unix_ms(), 1_500);
        let before = UNIX_EPOCH - Duration::from_millis(1_500);
        assert_eq!(Timestamp::from_system_time(before).as_unix_ms(), -1_500);
    }

    #[test]
    fn arithmetic_saturates() {
        let t = Timestamp::from_unix_ms(1_000);
        assert_eq!(t.saturating_add(Duration::from_secs(1)).as_unix_ms(), 2_000);
        assert_eq!(
            t.saturating_sub(Duration::from_secs(2)).as_unix_ms(),
            -1_000
        );
        assert_eq!(t.saturating_add(Duration::MAX).as_unix_ms(), i64::MAX);
        assert_eq!(
            t.saturating_duration_since(Timestamp::UNIX_EPOCH),
            Duration::from_secs(1)
        );
        assert_eq!(
            t.saturating_duration_since(t.saturating_add(Duration::from_secs(1))),
            Duration::ZERO
        );
    }

    #[test]
    fn serializes_as_unix_ms() {
        let t = Timestamp::from_unix_ms(1_792_622_700_042);
        assert_eq!(serde_json::to_string(&t).unwrap(), "1792622700042");
        assert_eq!(
            serde_json::from_str::<Timestamp>("1792622700042").unwrap(),
            t
        );
    }
}
