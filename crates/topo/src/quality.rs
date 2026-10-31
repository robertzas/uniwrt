//! Link quality on one 0–100 scale, whatever measures it, and traffic rates between builds.

use crate::{Traffic, links::Sample};

/// A signal's quality: 0 at −90 dBm and below, 100 at −50 dBm and above, linear between, so the
/// usual bands fall on quarters (−60 dBm is 75, −70 is 50, −80 is 25).
pub(crate) fn signal_quality(dbm: i16) -> u8 {
    let quality = ((i32::from(dbm) + 90) * 100 / 40).clamp(0, 100);
    u8::try_from(quality).unwrap_or(0)
}

/// batman-adv's transmit quality (0–255) as a share.
pub(crate) fn tq_quality(tq: u8) -> u8 {
    u8::try_from(u16::from(tq) * 100 / 255).unwrap_or(100)
}

/// The traffic of a link oriented `a` → `b`, from a reading by one end (`reversed` when that end
/// is `b`) and the edge's traffic in the previous build, wherever it was oriented then.
pub(crate) fn traffic(
    sample: Sample,
    reversed: bool,
    previous: Option<(&Traffic, bool)>,
) -> Traffic {
    let (down_bytes, up_bytes) = if reversed {
        (sample.in_bytes, sample.out_bytes)
    } else {
        (sample.out_bytes, sample.in_bytes)
    };
    let mut traffic = Traffic {
        at: sample.at,
        down_bytes,
        up_bytes,
        down_bps: None,
        up_bps: None,
    };
    let Some((before, flipped)) = previous else {
        return traffic;
    };
    let (down_before, up_before, down_bps, up_bps) = if flipped {
        (
            before.up_bytes,
            before.down_bytes,
            before.up_bps,
            before.down_bps,
        )
    } else {
        (
            before.down_bytes,
            before.up_bytes,
            before.down_bps,
            before.up_bps,
        )
    };
    if before.at == sample.at {
        // The same reading: its rates stand.
        if (down_before, up_before) == (down_bytes, up_bytes) {
            (traffic.down_bps, traffic.up_bps) = (down_bps, up_bps);
        }
    } else if before.at < sample.at {
        let ms = sample.at.saturating_duration_since(before.at).as_millis();
        traffic.down_bps = rate(down_before, down_bytes, ms);
        traffic.up_bps = rate(up_before, up_bytes, ms);
    }
    traffic
}

/// Bits per second; `None` when the counter went back (it restarted).
fn rate(before: u64, now: u64, ms: u128) -> Option<u64> {
    let bytes = now.checked_sub(before)?;
    u64::try_from(u128::from(bytes) * 8000 / ms).ok()
}

#[cfg(test)]
mod tests {
    use uniwrt_core::Timestamp;

    use super::*;

    #[test]
    fn signals_fall_on_quarters() {
        let qualities: Vec<_> = [-40, -50, -60, -70, -80, -90, -100]
            .into_iter()
            .map(signal_quality)
            .collect();
        assert_eq!(qualities, [100, 100, 75, 50, 25, 0, 0]);
        assert_eq!(
            (tq_quality(255), tq_quality(0), tq_quality(128)),
            (100, 0, 50)
        );
    }

    fn sample(ms: i64, out_bytes: u64, in_bytes: u64) -> Sample {
        Sample {
            at: Timestamp::from_unix_ms(ms),
            out_bytes,
            in_bytes,
        }
    }

    #[test]
    fn rates_come_from_the_previous_reading() {
        let first = traffic(sample(1_000, 1_000, 500), false, None);
        assert_eq!((first.down_bps, first.up_bps), (None, None));
        let second = traffic(sample(3_000, 3_000, 750), false, Some((&first, false)));
        assert_eq!((second.down_bps, second.up_bps), (Some(8_000), Some(1_000)));
        // Rebuilt without a new reading: the rates stay.
        let again = traffic(sample(3_000, 3_000, 750), false, Some((&second, false)));
        assert_eq!(again, second);
        // Read by the other end, or oriented the other way since: still the same directions.
        let reversed = traffic(sample(5_000, 1_250, 5_000), true, Some((&second, false)));
        assert_eq!(
            (reversed.down_bps, reversed.up_bps),
            (Some(8_000), Some(2_000))
        );
        let flipped = Traffic {
            down_bytes: 750,
            up_bytes: 3_000,
            ..second
        };
        let after_flip = traffic(sample(5_000, 5_000, 1_250), false, Some((&flipped, true)));
        assert_eq!(
            (after_flip.down_bps, after_flip.up_bps),
            (Some(8_000), Some(2_000))
        );
    }

    #[test]
    fn a_counter_that_restarted_has_no_rate() {
        let first = traffic(sample(1_000, 9_000, 9_000), false, None);
        let rebooted = traffic(sample(2_000, 100, 9_100), false, Some((&first, false)));
        assert_eq!((rebooted.down_bps, rebooted.up_bps), (None, Some(800)));
        let older = traffic(sample(500, 0, 0), false, Some((&first, false)));
        assert_eq!((older.down_bps, older.up_bps), (None, None));
    }
}
