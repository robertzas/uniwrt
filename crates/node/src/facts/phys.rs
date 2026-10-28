//! The `phys` set: nl80211's `GET_WIPHY`, trimmed by the agent. `bands` is indexed by `enum
//! nl80211_band`, with null where the phy lacks a band. A band's standards are the capability
//! fields it has; its widths come from their channel-width bits, and its spatial streams from the
//! newest standard's MCS set.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use uniwrt_model::{
    facts::{Channel, DfsState, PhyBand, PhyFacts},
    wifi::{Band, ChannelWidth, WifiStandard},
};

use super::wifi;

#[derive(Deserialize)]
pub(super) struct Raw {
    wiphys: Vec<Wiphy>,
}

#[derive(Deserialize)]
struct Wiphy {
    name: String,
    #[serde(default)]
    bands: Vec<Option<RawBand>>,
}

#[derive(Deserialize)]
struct RawBand {
    #[serde(default)]
    freqs: Vec<Freq>,
    ht_capa: Option<u32>,
    ht_mcs_set: Option<HtMcs>,
    vht_capa: Option<u32>,
    vht_mcs_set: Option<McsSet>,
    #[serde(default)]
    iftype_data: Vec<IftypeData>,
}

#[derive(Deserialize)]
struct Freq {
    #[serde(rename = "freq")]
    mhz: u32,
    max_tx_power: Option<u32>,
    #[serde(default)]
    disabled: bool,
    #[serde(default)]
    no_ir: bool,
    #[serde(default)]
    radar: bool,
    dfs_state: Option<u32>,
}

#[derive(Deserialize)]
struct HtMcs {
    #[serde(default)]
    rx_mcs_indexes: Vec<u8>,
}

#[derive(Deserialize)]
struct McsSet {
    #[serde(default)]
    rx_mcs_set: Vec<Streams>,
}

#[derive(Deserialize)]
struct Streams {
    streams: u8,
    #[serde(default)]
    mcs_indexes: Vec<u8>,
}

/// HE and EHT capabilities, per set of interface types.
#[derive(Deserialize)]
struct IftypeData {
    #[serde(default)]
    iftypes: BTreeMap<String, bool>,
    #[serde(default)]
    he_cap_phy: Vec<u8>,
    #[serde(default)]
    he_cap_mcs_set: Vec<McsSet>,
    #[serde(default)]
    eht_cap_phy: Vec<u8>,
}

// Channel-width bits: HT and VHT capability info, and the first octet of the HE and EHT PHY
// capabilities.
const HT_40: u32 = 0x0002;
const VHT_160: u32 = 0x000c;
const HE_40_IN_2G: u8 = 0x02;
const HE_40_80: u8 = 0x04;
const HE_160: u8 = 0x18;
const EHT_320_IN_6G: u8 = 0x02;

pub(super) fn parse(raw: Raw) -> Vec<PhyFacts> {
    raw.wiphys
        .into_iter()
        .map(|wiphy| {
            let mut bands: Vec<PhyBand> = wiphy
                .bands
                .into_iter()
                .enumerate()
                .filter_map(|(index, raw)| Some(phy_band(wifi::band(index)?, &raw?)))
                .collect();
            bands.sort_by_key(|band| band.band);
            PhyFacts {
                name: wiphy.name,
                bands,
            }
        })
        .collect()
}

fn phy_band(band: Band, raw: &RawBand) -> PhyBand {
    // An AP's capabilities, which may differ from a station's.
    let data = raw
        .iftype_data
        .iter()
        .find(|data| data.iftypes.get("ap") == Some(&true))
        .or(raw.iftype_data.first());
    let he = data.and_then(|data| data.he_cap_phy.first().copied());
    let eht = data.and_then(|data| data.eht_cap_phy.first().copied());

    let mut standards = BTreeSet::new();
    if matches!(band, Band::G2 | Band::G5) {
        standards.insert(WifiStandard::Legacy);
    }
    for (standard, has) in [
        (WifiStandard::Ht, raw.ht_capa.is_some()),
        (WifiStandard::Vht, raw.vht_capa.is_some()),
        (WifiStandard::He, he.is_some()),
        (WifiStandard::Eht, eht.is_some()),
    ] {
        if has {
            standards.insert(standard);
        }
    }

    let mut widths = BTreeSet::from([ChannelWidth::Mhz20]);
    let ht = raw.ht_capa.unwrap_or(0);
    let vht = raw.vht_capa.unwrap_or(0);
    let he = he.unwrap_or(0);
    if ht & HT_40 != 0 || (band == Band::G2 && he & HE_40_IN_2G != 0) {
        widths.insert(ChannelWidth::Mhz40);
    }
    if raw.vht_capa.is_some() || (band != Band::G2 && he & HE_40_80 != 0) {
        widths.extend([ChannelWidth::Mhz40, ChannelWidth::Mhz80]);
    }
    if vht & VHT_160 != 0 || (band != Band::G2 && he & HE_160 != 0) {
        widths.insert(ChannelWidth::Mhz160);
    }
    if band == Band::G6 && eht.unwrap_or(0) & EHT_320_IN_6G != 0 {
        widths.insert(ChannelWidth::Mhz320);
    }

    let he_streams = data
        .filter(|data| !data.he_cap_phy.is_empty())
        .and_then(|data| data.he_cap_mcs_set.iter().filter_map(streams).max());
    let legacy_ht_streams = || {
        let set = raw.ht_mcs_set.as_ref()?;
        // MCS 0–31 are 1–4 streams of 8 modulations each; MCS 32 and up are special.
        set.rx_mcs_indexes
            .iter()
            .filter(|&&mcs| mcs < 32)
            .max()
            .map(|mcs| mcs / 8 + 1)
    };
    let spatial_streams = he_streams
        .or_else(|| raw.vht_mcs_set.as_ref().and_then(streams))
        .or_else(legacy_ht_streams);

    PhyBand {
        band,
        standards,
        widths,
        spatial_streams,
        channels: raw
            .freqs
            .iter()
            .filter(|freq| !freq.disabled)
            .filter_map(|freq| channel(band, freq))
            .collect(),
    }
}

/// The most streams with any MCS in a receive set.
fn streams(set: &McsSet) -> Option<u8> {
    set.rx_mcs_set
        .iter()
        .filter(|streams| !streams.mcs_indexes.is_empty())
        .map(|streams| streams.streams)
        .max()
}

fn channel(band: Band, freq: &Freq) -> Option<Channel> {
    Some(Channel {
        number: wifi::channel(band, freq.mhz)?,
        frequency_mhz: freq.mhz,
        dfs: freq.radar,
        // `enum nl80211_dfs_state`
        dfs_state: match freq.dfs_state {
            Some(0) if freq.radar => Some(DfsState::Usable),
            Some(1) if freq.radar => Some(DfsState::Unavailable),
            Some(2) if freq.radar => Some(DfsState::Available),
            _ => None,
        },
        no_ir: freq.no_ir,
        max_tx_power_dbm: freq.max_tx_power.and_then(wifi::dbm),
    })
}
