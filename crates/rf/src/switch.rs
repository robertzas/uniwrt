//! A channel switch announcement (features/wifi.md, RF planner: Apply; M6.1 b1b, D-113): the
//! arguments of hostapd's `switch_chan` (`hostapd.<ifname>`) that move a running radio to a
//! primary channel and width of the band's standard blocks, as OpenWrt lays them out from a
//! primary channel and `htmode` ([`crate::plan`]'s blocks). Pure.

use serde::Serialize;
use uniwrt_model::wifi::{Band, ChannelWidth, WifiStandard};

use crate::plan::blocks;

/// How many beacons announce the switch before it happens (about a second at 100 TU).
pub const BEACONS: u32 = 10;

/// `switch_chan`'s arguments, serialized as hostapd takes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Switch {
    /// The primary channel's frequency, MHz.
    pub freq: u32,
    pub bcn_count: u32,
    /// The center of the whole channel, MHz (the primary's at 20 MHz).
    pub center_freq1: u32,
    /// MHz: 20, 40, 80 or 160.
    pub bandwidth: u16,
    /// Where the secondary 20 MHz lies beside the primary: 1 above, −1 below, 0 at 20 MHz.
    pub sec_channel_offset: i8,
    pub ht: bool,
    pub vht: bool,
    pub he: bool,
}

/// The switch to `channel` at `width` on `band` for a radio of `standard`; `None` where a
/// switch can't say it: 320 MHz and Wi-Fi 7 (the call has no EHT flag), 60 GHz, a width the
/// standard lacks, or a channel outside the band's blocks of that width (2.4 GHz above 20 MHz).
pub fn switch(
    band: Band,
    channel: u16,
    width: ChannelWidth,
    standard: WifiStandard,
) -> Option<Switch> {
    let widest = match standard {
        WifiStandard::Legacy => ChannelWidth::Mhz20,
        WifiStandard::Ht => ChannelWidth::Mhz40,
        WifiStandard::Vht | WifiStandard::He => ChannelWidth::Mhz160,
        WifiStandard::Eht => return None,
    };
    if width > widest || band == Band::G60 || (band == Band::G6 && standard != WifiStandard::He) {
        return None;
    }
    let freq = band.frequency(channel);
    let (center_freq1, sec_channel_offset) = if width == ChannelWidth::Mhz20 {
        if !blocks::blocks(band, width)
            .iter()
            .any(|b| b.channels == [channel])
            && band != Band::G2
        {
            return None;
        }
        (freq, 0)
    } else {
        let block = blocks::blocks(band, width)
            .into_iter()
            .find(|b| b.channels.contains(&channel))?;
        let at = block.channels.iter().position(|c| *c == channel)?;
        let span = block.span(band);
        (
            u32::midpoint(span.low, span.high),
            if at % 2 == 0 { 1 } else { -1 },
        )
    };
    let he = standard == WifiStandard::He;
    Some(Switch {
        freq,
        bcn_count: BEACONS,
        center_freq1,
        bandwidth: width.mhz(),
        sec_channel_offset,
        ht: standard != WifiStandard::Legacy && band != Band::G6,
        vht: matches!(standard, WifiStandard::Vht | WifiStandard::He) && band == Band::G5,
        he,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(band: Band, channel: u16, width: u16, standard: WifiStandard) -> Option<Switch> {
        switch(
            band,
            channel,
            ChannelWidth::try_from(width).unwrap(),
            standard,
        )
    }

    #[test]
    fn five_ghz_blocks_center_and_offset() {
        let s = at(Band::G5, 44, 80, WifiStandard::He).unwrap();
        assert_eq!((s.freq, s.center_freq1, s.bandwidth), (5220, 5210, 80));
        assert_eq!(s.sec_channel_offset, 1);
        assert_eq!(
            at(Band::G5, 48, 80, WifiStandard::He)
                .unwrap()
                .sec_channel_offset,
            -1
        );
        assert!(s.ht && s.vht && s.he);
        let s = at(Band::G5, 149, 40, WifiStandard::Vht).unwrap();
        assert_eq!((s.center_freq1, s.sec_channel_offset), (5755, 1));
        assert!(s.vht && !s.he);
        let s = at(Band::G5, 100, 160, WifiStandard::He).unwrap();
        assert_eq!((s.center_freq1, s.sec_channel_offset), (5570, 1));
        assert_eq!(
            at(Band::G5, 36, 20, WifiStandard::Ht).unwrap().center_freq1,
            5180
        );
    }

    #[test]
    fn two_ghz_is_20_mhz_on_any_channel() {
        let s = at(Band::G2, 6, 20, WifiStandard::He).unwrap();
        assert_eq!(
            (s.freq, s.center_freq1, s.sec_channel_offset),
            (2437, 2437, 0)
        );
        assert!(s.ht && !s.vht && s.he);
        assert!(at(Band::G2, 3, 20, WifiStandard::Ht).is_some());
        assert_eq!(at(Band::G2, 6, 40, WifiStandard::Ht), None);
        assert!(!at(Band::G2, 1, 20, WifiStandard::Legacy).unwrap().ht);
    }

    #[test]
    fn six_ghz_is_he_without_ht() {
        let s = at(Band::G6, 37, 160, WifiStandard::He).unwrap();
        assert_eq!(
            (s.freq, s.center_freq1, s.sec_channel_offset),
            (6135, 6185, -1)
        );
        assert!(!s.ht && !s.vht && s.he);
    }

    #[test]
    fn what_a_switch_cannot_say() {
        assert_eq!(at(Band::G6, 1, 320, WifiStandard::He), None);
        assert_eq!(at(Band::G5, 36, 80, WifiStandard::Eht), None);
        assert_eq!(at(Band::G5, 36, 80, WifiStandard::Ht), None);
        assert_eq!(at(Band::G5, 144, 160, WifiStandard::Vht), None);
        assert_eq!(at(Band::G5, 38, 20, WifiStandard::Vht), None);
    }
}
