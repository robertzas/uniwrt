//! nl80211's vocabulary, shared by the Wi-Fi parsers: bands, channel numbers, widths and powers.

use uniwrt_model::wifi::{Band, ChannelWidth};

/// `enum nl80211_band`, by which a phy's `wiphy_bands` are indexed.
pub(super) fn band(index: usize) -> Option<Band> {
    match index {
        0 => Some(Band::G2),
        1 => Some(Band::G5),
        2 => Some(Band::G60),
        3 => Some(Band::G6),
        // S1G (sub-1 GHz) and light communication
        _ => None,
    }
}

/// UCI's names of the bands.
pub(super) fn band_named(name: &str) -> Option<Band> {
    match name {
        "2g" => Some(Band::G2),
        "5g" => Some(Band::G5),
        "6g" => Some(Band::G6),
        "60g" => Some(Band::G60),
        _ => None,
    }
}

/// `enum nl80211_chan_width`; 5 and 10 MHz and the S1G widths have no [`ChannelWidth`].
pub(super) fn width(value: u32) -> Option<ChannelWidth> {
    Some(match value {
        // 20 MHz without HT, and with it
        0 | 1 => ChannelWidth::Mhz20,
        2 => ChannelWidth::Mhz40,
        3 => ChannelWidth::Mhz80,
        // 80+80 and 160 MHz
        4 | 5 => ChannelWidth::Mhz160,
        13 => ChannelWidth::Mhz320,
        _ => return None,
    })
}

/// A power in mBm, as the kernel reports it, in whole dBm.
pub(super) fn dbm(mbm: u32) -> Option<i16> {
    i16::try_from(mbm / 100).ok()
}

#[cfg(test)]
mod tests {
    use uniwrt_model::wifi::ChannelWidth;

    use super::{dbm, width};

    #[test]
    fn widths_and_powers_convert() {
        assert_eq!(width(1), Some(ChannelWidth::Mhz20));
        assert_eq!(width(4), Some(ChannelWidth::Mhz160));
        assert_eq!(width(13), Some(ChannelWidth::Mhz320));
        assert_eq!(width(6), None);
        assert_eq!(dbm(2300), Some(23));
    }
}
