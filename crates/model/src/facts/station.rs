//! Associated stations (Wi-Fi clients and mesh peers): signal, rates, traffic and capabilities.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use uniwrt_core::MacAddr;

use crate::wifi::{ChannelWidth, WifiStandard};

/// A station associated with one of the node's Wi-Fi interfaces. Directions are the node's:
/// `rx` is traffic from the station, `tx` traffic to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StationFacts {
    pub mac: MacAddr,
    /// The Wi-Fi interface it is associated with, such as `phy1-ap0`.
    pub iface: String,
    /// Average signal.
    pub signal_dbm: Option<i16>,
    pub connected_s: u64,
    pub inactive_ms: u64,
    /// Rate of the last frame received from the station.
    pub rx_rate: Option<Rate>,
    /// Rate of the last frame sent to the station.
    pub tx_rate: Option<Rate>,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub tx_retries: Option<u64>,
    pub tx_failed: Option<u64>,
    /// The driver's estimate of achievable throughput to the station.
    pub expected_throughput_kbps: Option<u32>,
    /// Passed authentication (802.1X or the 4-way handshake).
    pub authorized: bool,
    /// The newest standard the station supports, which may be newer than its current rate's.
    pub standard: Option<WifiStandard>,
    pub capabilities: BTreeSet<StationCapability>,
}

/// A PHY rate with the parameters it was sent with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rate {
    pub kbps: u32,
    pub standard: WifiStandard,
    pub mcs: Option<u8>,
    /// Spatial streams.
    pub nss: Option<u8>,
    pub width: Option<ChannelWidth>,
    /// 400 or 800 ns (HT, VHT); 800, 1600 or 3200 ns (HE, EHT).
    pub guard_interval_ns: Option<u16>,
}

/// A roaming or protection feature the station advertised, serialized as its 802.11 amendment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum StationCapability {
    /// 802.11k radio measurement.
    #[serde(rename = "11k")]
    RadioMeasurement,
    /// 802.11r fast transition (the station associated with FT).
    #[serde(rename = "11r")]
    FastTransition,
    /// 802.11v BSS transition management.
    #[serde(rename = "11v")]
    BssTransition,
    /// 802.11w protected management frames.
    #[serde(rename = "11w")]
    ProtectedManagementFrames,
    /// Wi-Fi Multimedia (QoS).
    #[serde(rename = "wmm")]
    Wmm,
}
