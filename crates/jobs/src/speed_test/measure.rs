//! What the speed test's script prints, read (pure): the WAN device's byte counters each
//! second while loading, and the ping replies; rates from the counters past the warm-up (TCP's
//! slow start), the median round trip idle and the 90th percentile under load.

use serde::Deserialize;

/// A round trip that got no reply under load counts as this, in µs: a lost ping is the worst
/// bufferbloat there is.
pub(crate) const LOST_US: u32 = 1_000_000;
/// The seconds of a load phase left out of its rate.
pub(crate) const WARMUP_CS: u64 = 200;

/// One line the script prints.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Line {
    Sample { t: u64, rx: u64, tx: u64 },
    Rtt { rtt: f64 },
    Err { err: String },
}

/// The WAN device's counters at a moment, in centiseconds of uptime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Sample {
    pub(crate) t: u64,
    pub(crate) rx: u64,
    pub(crate) tx: u64,
}

/// One run of the script.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Phase {
    pub(crate) samples: Vec<Sample>,
    /// The ping replies, in µs, in order.
    pub(crate) rtts_us: Vec<u32>,
    pub(crate) err: Option<String>,
}

pub(crate) fn parse(stdout: &[u8]) -> Phase {
    let mut phase = Phase::default();
    for line in String::from_utf8_lossy(stdout).lines() {
        match serde_json::from_str::<Line>(line) {
            Ok(Line::Sample { t, rx, tx }) => phase.samples.push(Sample { t, rx, tx }),
            Ok(Line::Rtt { rtt }) if rtt.is_finite() && rtt >= 0.0 => {
                // Within u32: a reply after more than an hour isn't one.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                phase
                    .rtts_us
                    .push((rtt * 1000.0).round().min(f64::from(u32::MAX)) as u32);
            }
            Ok(Line::Err { err }) => phase.err = Some(err),
            Ok(Line::Rtt { .. }) | Err(_) => {}
        }
    }
    phase
}

/// Which counter a phase loads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Direction {
    Down,
    Up,
}

/// The rate past the warm-up, in kbit/s; `None` without two samples to measure between.
pub(crate) fn rate_kbps(samples: &[Sample], direction: Direction) -> Option<u32> {
    let first = samples.first()?;
    let start = samples
        .iter()
        .find(|s| s.t >= first.t + WARMUP_CS)
        .unwrap_or(first);
    let end = samples.last()?;
    let cs = end.t.checked_sub(start.t).filter(|&cs| cs > 0)?;
    let bytes = match direction {
        Direction::Down => end.rx.checked_sub(start.rx)?,
        Direction::Up => end.tx.checked_sub(start.tx)?,
    };
    // bytes × 8 bits / (cs / 100) s / 1000 = bytes × 8 / (cs × 10) kbit/s.
    u32::try_from(u128::from(bytes) * 8 / (u128::from(cs) * 10)).ok()
}

/// The median of the replies; `None` without any.
pub(crate) fn median(rtts: &[u32]) -> Option<u32> {
    let mut sorted = rtts.to_vec();
    sorted.sort_unstable();
    sorted.get(sorted.len().checked_sub(1)? / 2).copied()
}

/// The 90th percentile (nearest rank) of `expected` round trips, those without a reply counted
/// as [`LOST_US`].
pub(crate) fn p90(rtts: &[u32], expected: usize) -> u32 {
    let mut all = rtts.to_vec();
    all.resize(expected.max(rtts.len()), LOST_US);
    all.sort_unstable();
    if all.is_empty() {
        return LOST_US;
    }
    let rank = (all.len() * 9).div_ceil(10);
    all[rank.saturating_sub(1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scripts_lines_parse() {
        let out = b"{\"t\":1000,\"rx\":0,\"tx\":5}\n{\"rtt\":12.345}\nPING noise\n\
                    {\"rtt\":-1}\n{\"err\":\"no_fetch\"}\n";
        let phase = parse(out);
        assert_eq!(
            phase.samples,
            [Sample {
                t: 1000,
                rx: 0,
                tx: 5
            }]
        );
        assert_eq!(phase.rtts_us, [12_345]);
        assert_eq!(phase.err.as_deref(), Some("no_fetch"));
    }

    #[test]
    fn rates_leave_the_warm_up_out() {
        // 1 s of slow start at 10 Mbit/s, then 100 Mbit/s (12.5 MB a second).
        let mut samples = vec![
            Sample { t: 0, rx: 0, tx: 0 },
            Sample {
                t: 100,
                rx: 1_250_000,
                tx: 0,
            },
        ];
        for i in 1..=10u64 {
            samples.push(Sample {
                t: 100 + i * 100,
                rx: 1_250_000 + i * 12_500_000,
                tx: i * 1_000,
            });
        }
        assert_eq!(rate_kbps(&samples, Direction::Down), Some(100_000));
        assert_eq!(rate_kbps(&samples, Direction::Up), Some(8));
        assert_eq!(rate_kbps(&samples[..1], Direction::Down), None);
        // A counter that went backwards (the device came back) measures nothing.
        let reset = [
            Sample { t: 0, rx: 9, tx: 0 },
            Sample {
                t: 300,
                rx: 1,
                tx: 0,
            },
        ];
        assert_eq!(rate_kbps(&reset, Direction::Down), None);
    }

    #[test]
    fn latency_statistics() {
        assert_eq!(median(&[30, 10, 20]), Some(20));
        assert_eq!(median(&[10, 20]), Some(10));
        assert_eq!(median(&[]), None);
        let rtts: Vec<u32> = (1..=10).map(|i| i * 1_000).collect();
        assert_eq!(p90(&rtts, 10), 9_000);
        assert_eq!(p90(&rtts[..8], 10), LOST_US, "two lost of ten");
        assert_eq!(p90(&rtts[..9], 10), 9_000, "one lost of ten");
        assert_eq!(p90(&[], 0), LOST_US);
    }
}
