//! The frequencies a channel occupies, how much two channels share, and how much a signal counts.

use uniwrt_model::wifi::ChannelWidth;

/// The frequencies a channel occupies, in MHz: its center ± half its width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub low: u32,
    pub high: u32,
}

impl Span {
    pub fn new(center_mhz: u32, width: ChannelWidth) -> Self {
        let half = u32::from(width.mhz()) / 2;
        Self {
            low: center_mhz.saturating_sub(half),
            high: center_mhz + half,
        }
    }

    /// A 20 MHz channel.
    pub fn channel(frequency_mhz: u32) -> Self {
        Self::new(frequency_mhz, ChannelWidth::Mhz20)
    }

    pub const fn width(self) -> u32 {
        self.high - self.low
    }

    pub fn contains(self, mhz: u32) -> bool {
        (self.low..=self.high).contains(&mhz)
    }

    /// The share of this span that `other` covers, from 0 to 1. 2.4 GHz channels 5 MHz apart
    /// share three quarters; 1, 5, 9 and 13 share nothing.
    pub fn overlap(self, other: Self) -> f32 {
        let shared = self
            .high
            .min(other.high)
            .saturating_sub(self.low.max(other.low));
        if self.width() == 0 {
            return 0.0;
        }
        #[allow(clippy::cast_precision_loss)]
        let share = shared as f32 / self.width() as f32;
        share
    }
}

/// How much a transmitter heard at `signal_dbm` disturbs a channel it overlaps, from 0 to 1: a
/// logistic curve from about nothing at −90 dBm to about everything at −60 dBm
/// (features/wifi.md, the planner's `w(rssi)`).
pub fn weight(signal_dbm: i16) -> f32 {
    let x = (f32::from(signal_dbm) + 75.0) / 4.0;
    1.0 / (1.0 + (-x).exp())
}

#[cfg(test)]
mod tests {
    use uniwrt_model::wifi::ChannelWidth;

    use super::{Span, weight};

    #[test]
    fn channels_share_what_their_spans_share() {
        let ch1 = Span::channel(2412);
        assert_eq!(ch1.overlap(Span::channel(2417)), 0.75);
        assert_eq!(ch1.overlap(Span::channel(2432)), 0.0, "1 and 5");
        assert_eq!(ch1.overlap(ch1), 1.0);
        // An 80 MHz channel at 5210 covers 36 to 48, and a quarter of it is 36.
        let wide = Span::new(5210, ChannelWidth::Mhz80);
        assert_eq!(Span::channel(5180).overlap(wide), 1.0);
        assert_eq!(wide.overlap(Span::channel(5180)), 0.25);
        assert_eq!(wide.overlap(Span::channel(5260)), 0.0);
        assert!(wide.contains(5170) && !wide.contains(5260));
    }

    #[test]
    fn weight_rises_from_nothing_to_everything() {
        assert!(weight(-95) < 0.01);
        assert!((weight(-75) - 0.5).abs() < 1e-6);
        assert!(weight(-55) > 0.99);
        assert!(weight(-70) > weight(-80));
    }
}
