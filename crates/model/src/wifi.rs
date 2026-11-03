//! Wi-Fi vocabulary: bands, channel widths and 802.11 standards, named as OpenWrt names them.

use serde::{Deserialize, Serialize};

/// A frequency band, serialized as UCI's `band` option values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum Band {
    /// 2.4 GHz
    #[serde(rename = "2g")]
    G2,
    #[serde(rename = "5g")]
    G5,
    #[serde(rename = "6g")]
    G6,
    #[serde(rename = "60g")]
    G60,
}

impl Band {
    /// The band a frequency lies in.
    pub const fn of_frequency(mhz: u32) -> Option<Self> {
        match mhz {
            2400..=2500 => Some(Self::G2),
            4900..=5924 => Some(Self::G5),
            5925..=7125 => Some(Self::G6),
            57000..=72000 => Some(Self::G60),
            _ => None,
        }
    }

    /// The IEEE channel number of a frequency in this band.
    pub fn channel(self, mhz: u32) -> Option<u16> {
        let number = match self {
            Self::G2 if mhz == 2484 => 14,
            Self::G2 => mhz.checked_sub(2407)? / 5,
            Self::G5 => mhz.checked_sub(5000)? / 5,
            Self::G6 if mhz == 5935 => 2,
            Self::G6 => mhz.checked_sub(5950)? / 5,
            Self::G60 => mhz.checked_sub(56160)? / 2160,
        };
        u16::try_from(number).ok()
    }

    /// The center frequency of a 20 MHz channel (2.16 GHz on 60 GHz) of this band.
    pub fn frequency(self, channel: u16) -> u32 {
        let channel = u32::from(channel);
        match self {
            Self::G2 if channel == 14 => 2484,
            Self::G2 => 2407 + 5 * channel,
            Self::G5 => 5000 + 5 * channel,
            Self::G6 if channel == 2 => 5935,
            Self::G6 => 5950 + 5 * channel,
            Self::G60 => 56160 + 2160 * channel,
        }
    }
}

/// A channel width, serialized as its width in MHz.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(type = "20 | 40 | 80 | 160 | 320")
)]
#[serde(try_from = "u16", into = "u16")]
pub enum ChannelWidth {
    Mhz20,
    Mhz40,
    Mhz80,
    Mhz160,
    Mhz320,
}

impl ChannelWidth {
    pub const fn mhz(self) -> u16 {
        match self {
            Self::Mhz20 => 20,
            Self::Mhz40 => 40,
            Self::Mhz80 => 80,
            Self::Mhz160 => 160,
            Self::Mhz320 => 320,
        }
    }
}

impl From<ChannelWidth> for u16 {
    fn from(width: ChannelWidth) -> Self {
        width.mhz()
    }
}

impl TryFrom<u16> for ChannelWidth {
    type Error = String;

    fn try_from(mhz: u16) -> Result<Self, Self::Error> {
        Ok(match mhz {
            20 => Self::Mhz20,
            40 => Self::Mhz40,
            80 => Self::Mhz80,
            160 => Self::Mhz160,
            320 => Self::Mhz320,
            _ => return Err(format!("unsupported channel width {mhz} MHz")),
        })
    }
}

/// An 802.11 PHY standard, ordered from oldest to newest, serialized by its PHY name (the prefix
/// of UCI `htmode` values such as `HE80`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum WifiStandard {
    /// 802.11a/b/g
    Legacy,
    /// 802.11n, Wi-Fi 4
    Ht,
    /// 802.11ac, Wi-Fi 5
    Vht,
    /// 802.11ax, Wi-Fi 6/6E
    He,
    /// 802.11be, Wi-Fi 7
    Eht,
}

impl WifiStandard {
    /// The Wi-Fi Alliance generation number; legacy standards have none.
    pub const fn generation(self) -> Option<u8> {
        match self {
            Self::Legacy => None,
            Self::Ht => Some(4),
            Self::Vht => Some(5),
            Self::He => Some(6),
            Self::Eht => Some(7),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{from_str, json, to_value};

    use super::{Band, ChannelWidth, WifiStandard};

    #[test]
    fn serde_names_match_openwrt() {
        assert_eq!(
            to_value([Band::G2, Band::G5, Band::G6, Band::G60]).unwrap(),
            json!(["2g", "5g", "6g", "60g"])
        );
        assert_eq!(to_value(ChannelWidth::Mhz160).unwrap(), json!(160));
        assert_eq!(from_str::<ChannelWidth>("40").unwrap(), ChannelWidth::Mhz40);
        assert!(from_str::<ChannelWidth>("30").is_err());
        assert_eq!(to_value(WifiStandard::Vht).unwrap(), json!("vht"));
    }

    #[test]
    fn frequencies_map_to_channels_and_back() {
        let cases = [
            (2412, 1),
            (2484, 14),
            (5180, 36),
            (5825, 165),
            (5955, 1),
            (5935, 2),
            (6415, 93),
        ];
        for (mhz, number) in cases {
            let band = Band::of_frequency(mhz).unwrap();
            assert_eq!(band.channel(mhz), Some(number), "{mhz}");
            assert_eq!(band.frequency(number), mhz, "{number}");
        }
        assert_eq!(Band::of_frequency(902), None);
        assert_eq!(Band::G5.channel(4920), None);
    }

    #[test]
    fn standards_order_by_age() {
        assert!(WifiStandard::Legacy < WifiStandard::Ht && WifiStandard::He < WifiStandard::Eht);
        assert_eq!(WifiStandard::He.generation(), Some(6));
        assert_eq!(WifiStandard::Legacy.generation(), None);
    }
}
