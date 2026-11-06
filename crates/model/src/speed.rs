//! Speed tests from the gateway (features/wan.md, SQM; D-076): the line's rates and its
//! latency idle and under load, graded for bufferbloat, and what they suggest for SQM.

use serde::{Deserialize, Serialize};
use uniwrt_core::{JobId, NodeId, Timestamp};

use crate::site::gateway::Sqm;

/// One speed test's result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SpeedTest {
    /// The job that ran it.
    pub job: JobId,
    pub node: NodeId,
    pub at: Timestamp,
    /// Measured with SQM stopped: the line's own rates, which autotune shapes below.
    pub bypassed: bool,
    pub download_kbps: u32,
    pub upload_kbps: u32,
    /// The median round trip while the line is idle, in µs.
    pub idle_us: u32,
    /// The 90th percentile of round trips while downloading, in µs.
    pub download_us: u32,
    /// The same while uploading.
    pub upload_us: u32,
    pub grade: Grade,
}

/// How much latency grows under load (Waveform's scale): A+ under 5 ms, A under 30, B under 60,
/// C under 200, D under 400, F beyond.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum Grade {
    #[serde(rename = "A+")]
    APlus,
    A,
    B,
    C,
    D,
    F,
}

impl Grade {
    /// The grade of a line whose round trips grow from `idle_us` to `loaded_us` under load.
    pub const fn of(idle_us: u32, loaded_us: u32) -> Self {
        match loaded_us.saturating_sub(idle_us) {
            0..5_000 => Self::APlus,
            5_000..30_000 => Self::A,
            30_000..60_000 => Self::B,
            60_000..200_000 => Self::C,
            200_000..400_000 => Self::D,
            _ => Self::F,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::APlus => "A+",
            Self::A => "A",
            Self::B => "B",
            Self::C => "C",
            Self::D => "D",
            Self::F => "F",
        }
    }
}

/// The share of the line's measured rates SQM shapes to, in percent: downloads need more room
/// (the queue must build on the gateway, not at the ISP) than uploads.
pub const DOWNLOAD_SHARE: u32 = 90;
pub const UPLOAD_SHARE: u32 = 95;

/// How far the line may move from what the rates were tuned for, in percent, before autotune
/// suggests new ones.
pub const DRIFT_PERCENT: u32 = 10;

/// The rates a test of the line (SQM bypassed) suggests: [`DOWNLOAD_SHARE`] and
/// [`UPLOAD_SHARE`] of what it measured, `(download, upload)` in kbit/s.
pub fn suggest(test: &SpeedTest) -> (u32, u32) {
    (
        share(test.download_kbps, DOWNLOAD_SHARE),
        share(test.upload_kbps, UPLOAD_SHARE),
    )
}

fn share(kbps: u32, percent: u32) -> u32 {
    u32::try_from(u64::from(kbps) * u64::from(percent) / 100).unwrap_or(u32::MAX)
}

/// Whether a test of the line (SQM bypassed) suggests rates more than [`DRIFT_PERCENT`] away
/// from `sqm`'s: the line got faster or slower since they were set.
pub fn drifted(sqm: &Sqm, test: &SpeedTest) -> bool {
    if !test.bypassed {
        return false;
    }
    let (down, up) = suggest(test);
    let off = |now: u32, suggested: u32| {
        u64::from(now.abs_diff(suggested)) * 100 > u64::from(now) * u64::from(DRIFT_PERCENT)
    };
    off(sqm.download_kbps, down) || off(sqm.upload_kbps, up)
}

/// Autotune's re-check runs once a week, in the site's night (D-076).
pub const AUTOTUNE_EVERY_S: i64 = 7 * 24 * 3600;
/// The night's hours, local standard time: 03:00 to 06:00.
pub const NIGHT: std::ops::Range<i64> = 3..6;

/// Whether autotune's re-check is due at `now`: night at the site (`offset_s` east of UTC),
/// and a week since the last test of the line and the last try (a failed one waits too).
pub fn autotune_due(
    now: Timestamp,
    offset_s: i32,
    last_test: Option<Timestamp>,
    last_try: Option<Timestamp>,
) -> bool {
    let local_s = now.as_unix_ms().div_euclid(1000) + i64::from(offset_s);
    let night = NIGHT.contains(&local_s.rem_euclid(24 * 3600).div_euclid(3600));
    // A day less than a week, so the night window comes round each week.
    let waited = |at: Option<Timestamp>| {
        at.is_none_or(|at| {
            (now.as_unix_ms() - at.as_unix_ms()).div_euclid(1000) >= AUTOTUNE_EVERY_S - 86_400 / 2
        })
    };
    night && waited(last_test) && waited(last_try)
}

/// The speed tests the Internet page shows, and what the latest test of the line suggests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SpeedTests {
    /// Newest first.
    pub tests: Vec<SpeedTest>,
    pub suggestion: Option<Suggestion>,
}

/// SQM's rates as the latest test of the line (SQM bypassed) suggests them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Suggestion {
    /// The test it comes from.
    pub job: JobId,
    pub download_kbps: u32,
    pub upload_kbps: u32,
    /// The site's SQM is on with rates more than [`DRIFT_PERCENT`] away from these.
    pub drifted: bool,
}

impl SpeedTests {
    /// `tests`, newest first, with the suggestion of the newest that bypassed SQM, against the
    /// site's `sqm`.
    pub fn new(tests: Vec<SpeedTest>, sqm: Option<&Sqm>) -> Self {
        let suggestion = tests.iter().find(|test| test.bypassed).map(|test| {
            let (download_kbps, upload_kbps) = suggest(test);
            Suggestion {
                job: test.job,
                download_kbps,
                upload_kbps,
                drifted: sqm.is_some_and(|sqm| drifted(sqm, test)),
            }
        });
        Self { tests, suggestion }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test(download_kbps: u32, upload_kbps: u32) -> SpeedTest {
        SpeedTest {
            job: JobId::new(1),
            node: NodeId::new(1),
            at: Timestamp::from_unix_secs(1_800_000_000),
            bypassed: true,
            download_kbps,
            upload_kbps,
            idle_us: 12_000,
            download_us: 40_000,
            upload_us: 20_000,
            grade: Grade::A,
        }
    }

    #[test]
    fn autotune_runs_weekly_at_night() {
        // 2027-01-15 04:00 UTC; Berlin's standard time is an hour ahead.
        let four_utc = Timestamp::from_unix_secs(1_799_985_600);
        let hours = |h: i64| Timestamp::from_unix_ms(four_utc.as_unix_ms() + h * 3_600_000);
        assert!(autotune_due(four_utc, 0, None, None));
        assert!(autotune_due(four_utc, 3600, None, None), "05:00 in Berlin");
        assert!(
            !autotune_due(four_utc, 2 * 3600, None, None),
            "06:00 is morning"
        );
        assert!(!autotune_due(hours(12), 0, None, None), "noon");
        assert!(
            !autotune_due(four_utc, -5 * 3600, None, None),
            "23:00 in New York"
        );
        let days = |d: i64| Some(hours(-24 * d));
        assert!(
            !autotune_due(four_utc, 0, days(6), None),
            "tested this week"
        );
        assert!(autotune_due(four_utc, 0, days(7), days(7)));
        assert!(
            !autotune_due(four_utc, 0, days(7), days(1)),
            "tried yesterday"
        );
    }

    #[test]
    fn grades_follow_the_growth_under_load() {
        assert_eq!(Grade::of(12_000, 14_000), Grade::APlus);
        assert_eq!(Grade::of(12_000, 11_000), Grade::APlus);
        assert_eq!(Grade::of(12_000, 41_999), Grade::A);
        assert_eq!(Grade::of(12_000, 42_000), Grade::B);
        assert_eq!(Grade::of(10_000, 150_000), Grade::C);
        assert_eq!(Grade::of(10_000, 300_000), Grade::D);
        assert_eq!(Grade::of(10_000, 900_000), Grade::F);
        assert_eq!(serde_json::to_value(Grade::APlus).unwrap(), "A+");
        assert_eq!(Grade::APlus.label(), "A+");
    }

    #[test]
    fn autotune_suggests_a_share_and_notices_a_moved_line() {
        let line = test(1_000_000, 40_000);
        assert_eq!(suggest(&line), (900_000, 38_000));
        let mut sqm = Sqm {
            download_kbps: 900_000,
            upload_kbps: 38_000,
            overhead: None,
            autotune: true,
        };
        assert!(!drifted(&sqm, &line));
        sqm.download_kbps = 800_000;
        assert!(drifted(&sqm, &line), "the line got faster");
        sqm.download_kbps = 1_000_000;
        assert!(!drifted(&sqm, &line), "within 10% of the suggestion");
        sqm.download_kbps = 1_000_001;
        assert!(drifted(&sqm, &line));
        let shaped = SpeedTest {
            bypassed: false,
            ..test(500_000, 40_000)
        };
        assert!(
            !drifted(&sqm, &shaped),
            "a shaped test says nothing of the line"
        );
        assert_eq!(share(u32::MAX, 95), 4_080_218_930);
        let view = SpeedTests::new(vec![shaped.clone(), line.clone()], Some(&sqm));
        let suggestion = view.suggestion.unwrap();
        assert_eq!(
            (suggestion.download_kbps, suggestion.drifted),
            (900_000, true),
            "from the newest test of the line"
        );
        assert_eq!(SpeedTests::new(vec![shaped], None).suggestion, None);
    }
}
