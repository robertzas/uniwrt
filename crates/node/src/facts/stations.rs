//! The `stations` set: nl80211's station dump of every Wi-Fi interface, and for APs, what hostapd
//! saw in each client's association (`get_clients`). The capabilities a client advertised come
//! from hostapd; for mesh peers and an uplink's AP, from the kernel's station flags. Stations are
//! ordered by interface and MAC address.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use uniwrt_core::MacAddr;
use uniwrt_model::{
    facts::{PeerLinkState, Rate, StationCapability, StationFacts},
    wifi::{ChannelWidth, WifiStandard},
};

#[derive(Deserialize)]
pub(super) struct Raw {
    stations: BTreeMap<String, Vec<Station>>,
    #[serde(default)]
    clients: BTreeMap<String, BTreeMap<String, Client>>,
}

#[derive(Deserialize)]
struct Station {
    mac: String,
    #[serde(default)]
    info: Info,
}

#[derive(Default, Deserialize)]
struct Info {
    inactive_time: Option<u64>,
    connected_time: Option<u64>,
    signal: Option<i16>,
    signal_avg: Option<i16>,
    rx_bitrate: Option<Bitrate>,
    tx_bitrate: Option<Bitrate>,
    rx_bytes: Option<u64>,
    tx_bytes: Option<u64>,
    rx_bytes64: Option<u64>,
    tx_bytes64: Option<u64>,
    rx_packets: Option<u64>,
    tx_packets: Option<u64>,
    tx_retries: Option<u64>,
    tx_failed: Option<u64>,
    expected_throughput: Option<u32>,
    /// `struct nl80211_sta_flag_update`: the flags reported, and those set.
    #[serde(default)]
    sta_flags: Vec<u32>,
    plink_state: Option<u8>,
    rx_duration: Option<u64>,
    tx_duration: Option<u64>,
}

/// `NL80211_RATE_INFO_*`. The rate is in units of 100 kbit/s.
#[derive(Default, Deserialize)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "nl80211's flags, as the agent sends them"
)]
struct Bitrate {
    #[serde(rename = "bitrate")]
    rate: Option<u32>,
    #[serde(rename = "bitrate32")]
    rate32: Option<u32>,
    mcs: Option<u8>,
    vht_mcs: Option<u8>,
    vht_nss: Option<u8>,
    he_mcs: Option<u8>,
    he_nss: Option<u8>,
    he_gi: Option<u8>,
    eht_mcs: Option<u8>,
    eht_nss: Option<u8>,
    eht_gi: Option<u8>,
    #[serde(default)]
    short_gi: bool,
    #[serde(default, rename = "40_mhz_width")]
    mhz_40: bool,
    #[serde(default)]
    width_40: bool,
    #[serde(default)]
    width_80: bool,
    #[serde(default)]
    width_80p80: bool,
    #[serde(default)]
    width_160: bool,
    #[serde(default)]
    width_320: bool,
}

#[derive(Deserialize)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "hostapd's flags, as the agent sends them"
)]
struct Client {
    authorized: Option<bool>,
    #[serde(default)]
    wmm: bool,
    #[serde(default)]
    ht: bool,
    #[serde(default)]
    vht: bool,
    #[serde(default)]
    he: bool,
    #[serde(default)]
    eht: bool,
    #[serde(default)]
    mfp: bool,
    /// RRM enabled capabilities; any bit means 802.11k.
    #[serde(default)]
    rrm: Vec<u8>,
    #[serde(default)]
    extended_capabilities: Vec<u8>,
}

// `enum nl80211_sta_flags`
const FLAG_AUTHORIZED: u32 = 1 << 1;
const FLAG_WME: u32 = 1 << 3;
const FLAG_MFP: u32 = 1 << 4;
/// Extended capabilities bit 19: BSS transition management (802.11v).
const EXT_BSS_TRANSITION: (usize, u8) = (2, 1 << 3);

pub(super) fn parse(raw: Raw) -> Vec<StationFacts> {
    let Raw { stations, clients } = raw;
    let mut stations: Vec<StationFacts> = stations
        .iter()
        .flat_map(|(iface, stations)| {
            let clients = clients.get(iface);
            stations.iter().filter_map(move |station| {
                let client = clients.and_then(|clients| clients.get(&station.mac));
                facts(iface, station, client)
            })
        })
        .collect();
    stations.sort_by(|a, b| (&a.iface, a.mac.octets()).cmp(&(&b.iface, b.mac.octets())));
    stations
}

fn facts(iface: &str, station: &Station, client: Option<&Client>) -> Option<StationFacts> {
    let mac: MacAddr = station.mac.parse().ok()?;
    let info = &station.info;
    let flags = match info.sta_flags[..] {
        [mask, set] => mask & set,
        _ => 0,
    };
    let rx_rate = info.rx_bitrate.as_ref().and_then(rate);
    let tx_rate = info.tx_bitrate.as_ref().and_then(rate);
    let mut capabilities = BTreeSet::new();
    if flags & FLAG_WME != 0 || client.is_some_and(|c| c.wmm) {
        capabilities.insert(StationCapability::Wmm);
    }
    if flags & FLAG_MFP != 0 || client.is_some_and(|c| c.mfp) {
        capabilities.insert(StationCapability::ProtectedManagementFrames);
    }
    if let Some(client) = client {
        if client.rrm.iter().any(|&byte| byte != 0) {
            capabilities.insert(StationCapability::RadioMeasurement);
        }
        let (byte, bit) = EXT_BSS_TRANSITION;
        if client
            .extended_capabilities
            .get(byte)
            .is_some_and(|b| b & bit != 0)
        {
            capabilities.insert(StationCapability::BssTransition);
        }
    }
    // Without hostapd's view, the newest standard the station was seen using.
    let standard = client.map(supported).or_else(|| {
        [rx_rate, tx_rate]
            .into_iter()
            .flatten()
            .map(|rate| rate.standard)
            .max()
    });
    Some(StationFacts {
        mac,
        iface: iface.to_owned(),
        signal_dbm: info.signal_avg.or(info.signal),
        connected_s: info.connected_time.unwrap_or(0),
        inactive_ms: info.inactive_time.unwrap_or(0),
        rx_rate,
        tx_rate,
        rx_bytes: info.rx_bytes64.or(info.rx_bytes).unwrap_or(0),
        tx_bytes: info.tx_bytes64.or(info.tx_bytes).unwrap_or(0),
        rx_packets: info.rx_packets.unwrap_or(0),
        tx_packets: info.tx_packets.unwrap_or(0),
        tx_retries: info.tx_retries,
        tx_failed: info.tx_failed,
        expected_throughput_kbps: info.expected_throughput.filter(|&kbps| kbps > 0),
        rx_airtime_us: info.rx_duration,
        tx_airtime_us: info.tx_duration,
        peer_link: info.plink_state.and_then(peer_link),
        authorized: client
            .and_then(|client| client.authorized)
            .unwrap_or(flags & FLAG_AUTHORIZED != 0),
        standard,
        capabilities,
    })
}

/// The newest standard a client's association advertised.
fn supported(client: &Client) -> WifiStandard {
    [
        (client.eht, WifiStandard::Eht),
        (client.he, WifiStandard::He),
        (client.vht, WifiStandard::Vht),
        (client.ht, WifiStandard::Ht),
    ]
    .into_iter()
    .find_map(|(has, standard)| has.then_some(standard))
    .unwrap_or(WifiStandard::Legacy)
}

fn rate(bitrate: &Bitrate) -> Option<Rate> {
    let kbps = bitrate.rate32.or(bitrate.rate)?.checked_mul(100)?;
    let (standard, mcs, nss) = if let Some(mcs) = bitrate.eht_mcs {
        (WifiStandard::Eht, Some(mcs), bitrate.eht_nss)
    } else if let Some(mcs) = bitrate.he_mcs {
        (WifiStandard::He, Some(mcs), bitrate.he_nss)
    } else if let Some(mcs) = bitrate.vht_mcs {
        (WifiStandard::Vht, Some(mcs), bitrate.vht_nss)
    } else if let Some(mcs) = bitrate.mcs {
        // HT MCS 0–31 count 8 modulations per stream; MCS 32 and up are special.
        if mcs < 32 {
            (WifiStandard::Ht, Some(mcs % 8), Some(mcs / 8 + 1))
        } else {
            (WifiStandard::Ht, Some(mcs), None)
        }
    } else {
        (WifiStandard::Legacy, None, None)
    };
    let width = (standard != WifiStandard::Legacy).then_some(if bitrate.width_320 {
        ChannelWidth::Mhz320
    } else if bitrate.width_160 || bitrate.width_80p80 {
        ChannelWidth::Mhz160
    } else if bitrate.width_80 {
        ChannelWidth::Mhz80
    } else if bitrate.width_40 || bitrate.mhz_40 {
        ChannelWidth::Mhz40
    } else {
        ChannelWidth::Mhz20
    });
    let guard_interval_ns = match standard {
        WifiStandard::Legacy => None,
        WifiStandard::Ht | WifiStandard::Vht => Some(if bitrate.short_gi { 400 } else { 800 }),
        WifiStandard::He => he_guard_interval(bitrate.he_gi),
        WifiStandard::Eht => he_guard_interval(bitrate.eht_gi),
    };
    Some(Rate {
        kbps,
        standard,
        mcs,
        nss,
        width,
        guard_interval_ns,
    })
}

/// `enum nl80211_he_gi` (and `nl80211_eht_gi`, which has the same values).
fn he_guard_interval(gi: Option<u8>) -> Option<u16> {
    match gi.unwrap_or(0) {
        0 => Some(800),
        1 => Some(1600),
        2 => Some(3200),
        _ => None,
    }
}

/// `enum nl80211_plink_state`
fn peer_link(state: u8) -> Option<PeerLinkState> {
    Some(match state {
        0 => PeerLinkState::Listen,
        1 => PeerLinkState::OpenSent,
        2 => PeerLinkState::OpenReceived,
        3 => PeerLinkState::ConfirmReceived,
        4 => PeerLinkState::Established,
        5 => PeerLinkState::Holding,
        6 => PeerLinkState::Blocked,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use uniwrt_model::wifi::{ChannelWidth, WifiStandard};

    use super::{Bitrate, rate};

    fn parse(
        raw: serde_json::Value,
    ) -> (
        u32,
        WifiStandard,
        Option<u8>,
        Option<u8>,
        Option<u16>,
        Option<u16>,
    ) {
        let bitrate: Bitrate = serde_json::from_value(raw).unwrap();
        let rate = rate(&bitrate).unwrap();
        (
            rate.kbps,
            rate.standard,
            rate.mcs,
            rate.nss,
            rate.width.map(ChannelWidth::mhz),
            rate.guard_interval_ns,
        )
    }

    #[test]
    fn rates_carry_their_standard_and_parameters() {
        assert_eq!(
            parse(json!({"bitrate32": 722, "mcs": 15, "short_gi": true})),
            (
                72_200,
                WifiStandard::Ht,
                Some(7),
                Some(2),
                Some(20),
                Some(400)
            )
        );
        assert_eq!(
            parse(json!({"bitrate32": 1215, "vht_mcs": 6, "vht_nss": 1, "40_mhz_width": true})),
            (
                121_500,
                WifiStandard::Vht,
                Some(6),
                Some(1),
                Some(40),
                Some(800)
            )
        );
        assert_eq!(
            parse(
                json!({"bitrate32": 12010, "he_mcs": 11, "he_nss": 2, "he_gi": 0, "width_80": true})
            ),
            (
                1_201_000,
                WifiStandard::He,
                Some(11),
                Some(2),
                Some(80),
                Some(800)
            )
        );
        assert_eq!(
            parse(
                json!({"bitrate32": 57640, "eht_mcs": 13, "eht_nss": 2, "eht_gi": 1, "width_320": true})
            ),
            (
                5_764_000,
                WifiStandard::Eht,
                Some(13),
                Some(2),
                Some(320),
                Some(1600)
            )
        );
        assert_eq!(
            parse(json!({"bitrate": 10})),
            (1_000, WifiStandard::Legacy, None, None, None, None)
        );
        assert!(rate(&Bitrate::default()).is_none());
    }
}
