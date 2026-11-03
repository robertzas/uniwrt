//! Radar hits from hostapd's log (`DFS-RADAR-DETECTED`, the `dfs-radar` pattern): where the radar
//! was, so each channel's radar history can be told.

use uniwrt_core::{NodeId, Timestamp};
use uniwrt_model::wifi::ChannelWidth;

use crate::span::Span;

/// Radar detected by a node's radio over `span` (the whole channel it operated on).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RadarHit {
    pub node: NodeId,
    pub at: Timestamp,
    pub span: Span,
}

/// The channel a `DFS-RADAR-DETECTED` line names, such as
/// `phy1-ap0: DFS-RADAR-DETECTED freq=5500 ht_enabled=1 chan_offset=0 chan_width=3 cf1=5530 cf2=0`:
/// `chan_width` is hostapd's `enum chan_width` and `cf1` the center; without them, the 20 MHz
/// channel at `freq`.
pub fn span(text: &str) -> Option<Span> {
    let rest = &text[text.find("DFS-RADAR-DETECTED")?..];
    let field = |name: &str| -> Option<u32> {
        rest.split_whitespace()
            .find_map(|word| word.strip_prefix(name)?.strip_prefix('='))?
            .parse()
            .ok()
    };
    let freq = field("freq")?;
    let width = match field("chan_width") {
        Some(2) => ChannelWidth::Mhz40,
        // 80+80 counts as its first 80.
        Some(3 | 4) => ChannelWidth::Mhz80,
        Some(5) => ChannelWidth::Mhz160,
        _ => ChannelWidth::Mhz20,
    };
    let center = field("cf1").filter(|&cf1| cf1 != 0 && width != ChannelWidth::Mhz20);
    Some(Span::new(center.unwrap_or(freq), width))
}

#[cfg(test)]
mod tests {
    use super::span;
    use crate::span::Span;

    #[test]
    fn a_radar_line_names_the_whole_channel() {
        let line = "phy1-ap0: DFS-RADAR-DETECTED freq=5500 ht_enabled=1 chan_offset=0 \
                    chan_width=3 cf1=5530 cf2=0";
        assert_eq!(
            span(line),
            Some(Span {
                low: 5490,
                high: 5570
            })
        );
        let bare = "DFS-RADAR-DETECTED freq=5260 ht_enabled=0 chan_offset=0 chan_width=1";
        assert_eq!(
            span(bare),
            Some(Span {
                low: 5250,
                high: 5270
            })
        );
        assert_eq!(span("DFS-CAC-START freq=5260"), None);
        assert_eq!(span("DFS-RADAR-DETECTED freq=x"), None);
    }
}
