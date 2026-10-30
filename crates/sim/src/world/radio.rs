//! Radio arithmetic: bands and channels, indoor path loss, and the rates a signal-to-noise
//! ratio buys (MCS tables of 802.11n, ac and ax).

use super::catalog::Standard;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Band {
    G2,
    G5,
}

impl Band {
    pub(crate) const BOTH: [Self; 2] = [Self::G2, Self::G5];

    /// The radio, phy and AP interface of the band: radio0/phy0 is 2.4 GHz, radio1/phy1 5 GHz.
    pub(crate) const fn index(self) -> usize {
        match self {
            Self::G2 => 0,
            Self::G5 => 1,
        }
    }

    pub(crate) const fn ap_iface(self) -> &'static str {
        match self {
            Self::G2 => "phy0-ap0",
            Self::G5 => "phy1-ap0",
        }
    }

    pub(crate) const fn uci(self) -> &'static str {
        match self {
            Self::G2 => "2g",
            Self::G5 => "5g",
        }
    }

    pub(crate) const fn width_mhz(self) -> u32 {
        match self {
            Self::G2 => 20,
            Self::G5 => 80,
        }
    }

    pub(crate) fn freq(self, channel: u8) -> u32 {
        match self {
            Self::G2 => 2407 + 5 * u32::from(channel),
            Self::G5 => 5000 + 5 * u32::from(channel),
        }
    }

    /// The centre of the channel's block: itself at 20 MHz, the middle of its 80 MHz block on
    /// 5 GHz (36–48 → 42).
    pub(crate) fn center_freq(self, channel: u8) -> u32 {
        match self {
            Self::G2 => self.freq(channel),
            Self::G5 => {
                let first = if channel >= 149 {
                    149 + (channel - 149) / 16 * 16
                } else {
                    36 + (channel - 36) / 16 * 16
                };
                self.freq(first + 6)
            }
        }
    }

    /// Loss over `distance` metres indoors: free space at 1 m, a log-distance exponent of 3,
    /// and 0.5 dB per metre for walls and furniture.
    pub(crate) fn path_loss(self, distance: f64) -> f64 {
        let distance = distance.max(1.0);
        let at_one_metre = match self {
            Self::G2 => 40.0,
            Self::G5 => 46.4,
        };
        at_one_metre + 30.0 * distance.log10() + 0.5 * distance
    }
}

/// The SNR in dB each MCS needs, 0 to 11.
const SNR_FOR_MCS: [f64; 12] = [
    5.0, 8.0, 11.0, 14.0, 18.0, 21.0, 24.0, 27.0, 30.0, 32.0, 35.0, 38.0,
];
/// Mbit/s of one stream at 20 MHz: 802.11ax with 0.8 µs guard intervals, else 802.11n/ac with
/// the long one (MCS 9 at 20 MHz exists only in 11ax; 11ac uses it from 40 MHz up).
const HE_20: [f64; 12] = [
    8.6, 17.2, 25.8, 34.4, 51.6, 68.8, 77.4, 86.0, 103.2, 114.7, 129.0, 143.4,
];
const VHT_20: [f64; 10] = [6.5, 13.0, 19.5, 26.0, 39.0, 52.0, 58.5, 65.0, 78.0, 86.7];

/// A rate as nl80211 reports it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Rate {
    pub(crate) standard: Standard,
    pub(crate) mcs: u8,
    pub(crate) streams: u8,
    pub(crate) width_mhz: u32,
    /// In 100 kbit/s, nl80211's unit.
    pub(crate) bitrate: u32,
}

impl Rate {
    /// The fastest rate `snr` sustains for a client of `standard` with `streams` streams.
    pub(crate) fn for_snr(snr: f64, standard: Standard, streams: u8, width_mhz: u32) -> Self {
        let top: u8 = match standard {
            Standard::Ht => 7,
            Standard::Vht if width_mhz == 20 => 8,
            Standard::Vht => 9,
            Standard::He => 11,
        };
        let mcs = SNR_FOR_MCS
            .iter()
            .take(usize::from(top) + 1)
            .rposition(|&needed| snr >= needed)
            .unwrap_or(0) as u8;
        let base = match standard {
            Standard::He => HE_20[usize::from(mcs)],
            Standard::Vht | Standard::Ht => VHT_20[usize::from(mcs)],
        };
        let width = match (standard, width_mhz) {
            (_, 20) => 1.0,
            (Standard::He, 40) => 2.0,
            (Standard::He, _) => 4.19,
            (_, 40) => 2.077,
            (_, _) => 4.5,
        };
        Self {
            standard,
            mcs,
            streams,
            width_mhz,
            bitrate: (base * width * f64::from(streams) * 10.0).round() as u32,
        }
    }

    /// What the link carries in practice, in kbit/s (nl80211's `expected_throughput`).
    pub(crate) fn expected_kbps(self) -> u32 {
        self.bitrate * 65
    }
}

/// batman's link quality (0–255) for a signal in dBm: perfect above −60, nothing below −90.
pub(crate) fn tq(signal: f64) -> u8 {
    (255.0 * ((signal + 90.0) / 30.0).clamp(0.0, 1.0)).round() as u8
}

#[cfg(test)]
mod tests {
    use super::{Band, Rate, tq};
    use crate::world::catalog::Standard;

    #[test]
    fn channels_have_their_frequencies() {
        assert_eq!(Band::G2.freq(1), 2412);
        assert_eq!(Band::G2.freq(11), 2462);
        assert_eq!(Band::G5.freq(36), 5180);
        assert_eq!(Band::G5.center_freq(36), 5210);
        assert_eq!(Band::G5.center_freq(48), 5210);
        assert_eq!(Band::G5.center_freq(149), 5775);
    }

    #[test]
    fn rates_follow_the_signal() {
        let best = Rate::for_snr(50.0, Standard::He, 2, 80);
        assert_eq!((best.mcs, best.bitrate), (11, 12_017));
        let iot = Rate::for_snr(50.0, Standard::Ht, 1, 20);
        assert_eq!((iot.mcs, iot.bitrate), (7, 650));
        assert_eq!(Rate::for_snr(2.0, Standard::He, 1, 20).mcs, 0);
        assert!(Band::G5.path_loss(10.0) > Band::G2.path_loss(10.0));
        assert_eq!((tq(-50.0), tq(-75.0), tq(-95.0)), (255, 128, 0));
    }
}
