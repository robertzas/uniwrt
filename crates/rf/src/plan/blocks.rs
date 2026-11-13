//! The channels a radio may take: each band's standard blocks of 20 MHz channels per width (the
//! layout OpenWrt derives from a primary channel and `htmode`), filtered by what the radios allow
//! and the DFS policy.

use uniwrt_model::{
    facts::{Channel, DfsState},
    rf::DfsPolicy,
    wifi::{Band, ChannelWidth},
};

use crate::span::Span;

/// The weather radar's frequencies (channels 120–128), whose CAC lasts 10 minutes in the EU.
pub(crate) const WEATHER_MHZ: std::ops::RangeInclusive<u32> = 5590..=5660;

/// The 2.4 GHz channels that don't overlap: 1/6/11, or 1/5/9/13 where 13 is allowed.
pub(crate) const G2_SETS: [&[u16]; 2] = [&[1, 6, 11], &[1, 5, 9, 13]];

/// One block of 20 MHz channels of a width, in frequency order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Block {
    pub width: ChannelWidth,
    pub channels: Vec<u16>,
}

impl Block {
    pub(crate) fn span(&self, band: Band) -> Span {
        let first = band.frequency(self.channels[0]);
        let last = band.frequency(self.channels[self.channels.len() - 1]);
        Span::new(u32::midpoint(first, last), self.width)
    }
}

/// The band's standard blocks of `width`; 2.4 GHz has its non-overlapping 20 MHz channels only.
pub(crate) fn blocks(band: Band, width: ChannelWidth) -> Vec<Block> {
    let n = usize::from(width.mhz() / 20);
    let runs: Vec<Vec<u16>> = match band {
        Band::G2 => {
            return if n == 1 {
                let mut all: Vec<u16> = G2_SETS.iter().flat_map(|s| s.iter().copied()).collect();
                all.sort_unstable();
                all.dedup();
                all.into_iter()
                    .map(|c| Block {
                        width,
                        channels: vec![c],
                    })
                    .collect()
            } else {
                Vec::new()
            };
        }
        // Two runs, each bonded from its first channel: 36–144 and 149–177.
        Band::G5 => vec![
            (36..=144).step_by(4).collect(),
            (149..=177).step_by(4).collect(),
        ],
        Band::G6 => vec![(1..=233).step_by(4).collect()],
        Band::G60 => return Vec::new(),
    };
    let mut out = Vec::new();
    for run in &runs {
        let mut push = |offset: usize| {
            for chunk in run.get(offset..).unwrap_or_default().chunks_exact(n) {
                out.push(Block {
                    width,
                    channels: chunk.to_vec(),
                });
            }
        };
        push(0);
        // 320 MHz on 6 GHz comes in two layouts, the second shifted by 160 MHz.
        if band == Band::G6 && width == ChannelWidth::Mhz320 {
            push(n / 2);
        }
    }
    out
}

/// Whether every channel of `block` is one the radio may start a network on, the policy allows
/// (unless `pinned`) and no radar holds; on 2.4 GHz, one of `g2`, the set the band is planned
/// with.
pub(crate) fn usable(
    block: &Block,
    channels: &[Channel],
    dfs: DfsPolicy,
    pinned: bool,
    g2: Option<&[u16]>,
) -> bool {
    if let Some(set) = g2
        && !pinned
        && !set.contains(&block.channels[0])
    {
        return false;
    }
    block.channels.iter().all(|number| {
        channels.iter().any(|c| {
            c.number == *number
                && (!c.no_ir || c.dfs)
                && c.dfs_state != Some(DfsState::Unavailable)
                && (pinned
                    || !c.dfs
                    || match dfs {
                        DfsPolicy::Avoid => false,
                        DfsPolicy::NonWeather => !WEATHER_MHZ.contains(&c.frequency_mhz),
                        DfsPolicy::All => true,
                    })
        })
    })
}

/// The 2.4 GHz sets every radio of the band allows; 1/6/11 alone when none is whole.
pub(crate) fn g2_sets(radios: &[&[Channel]]) -> Vec<&'static [u16]> {
    let whole: Vec<&'static [u16]> = G2_SETS
        .iter()
        .copied()
        .filter(|set| {
            radios
                .iter()
                .all(|channels| set.iter().all(|c| has(channels, *c)))
        })
        .collect();
    if whole.is_empty() {
        vec![G2_SETS[0]]
    } else {
        whole
    }
}

fn has(channels: &[Channel], number: u16) -> bool {
    channels.iter().any(|c| c.number == number)
}

#[cfg(test)]
mod tests {
    use uniwrt_model::wifi::{Band, ChannelWidth};

    use super::blocks;

    fn firsts(band: Band, width: ChannelWidth) -> Vec<(u16, usize)> {
        blocks(band, width)
            .iter()
            .map(|b| (b.channels[0], b.channels.len()))
            .collect()
    }

    #[test]
    fn five_ghz_bonds_from_36_and_149() {
        let eighty = firsts(Band::G5, ChannelWidth::Mhz80);
        assert_eq!(
            eighty.iter().map(|f| f.0).collect::<Vec<_>>(),
            [36, 52, 68, 84, 100, 116, 132, 149, 165]
        );
        let wide = blocks(Band::G5, ChannelWidth::Mhz160);
        assert_eq!(wide[0].channels[0], 36);
        assert_eq!(wide[0].span(Band::G5).low, 5170);
        assert_eq!(wide[0].span(Band::G5).high, 5330);
        assert_eq!(wide[2].channels[0], 100);
        assert_eq!(
            blocks(Band::G5, ChannelWidth::Mhz80)[7].span(Band::G5).low,
            5735
        );
    }

    #[test]
    fn six_ghz_has_both_320_layouts_and_2g_only_20() {
        let wide = firsts(Band::G6, ChannelWidth::Mhz320);
        assert!(wide.contains(&(1, 16)) && wide.contains(&(33, 16)) && wide.contains(&(65, 16)));
        assert_eq!(blocks(Band::G2, ChannelWidth::Mhz40), []);
        assert_eq!(
            firsts(Band::G2, ChannelWidth::Mhz20)
                .iter()
                .map(|f| f.0)
                .collect::<Vec<_>>(),
            [1, 5, 6, 9, 11, 13]
        );
    }
}
