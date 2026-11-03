//! The RF report: per band, the site's radios, each channel's occupancy, the networks heard and
//! the inter-node RSSI matrix. The API sends it as it is.

use serde::{Deserialize, Serialize};
use uniwrt_core::{MacAddr, NodeId, Timestamp};
use uniwrt_model::{
    facts::DfsState,
    rf::Bss,
    wifi::{Band, ChannelWidth},
};

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct RfReport {
    /// In band order; a band appears once a radio uses it or a scan heard it.
    pub bands: Vec<BandReport>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct BandReport {
    pub band: Band,
    /// By node, then radio.
    pub radios: Vec<RadioReport>,
    /// The band's 20 MHz channels, in frequency order.
    pub channels: Vec<ChannelReport>,
    /// Every network the site's radios heard on the band, most disruptive first.
    pub networks: Vec<Neighbor>,
    /// How each node hears each other node on the band, by hearer then heard.
    pub matrix: Vec<MatrixEntry>,
}

/// One of the site's radios on the band.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct RadioReport {
    pub node: NodeId,
    pub radio: String,
    pub up: bool,
    pub channel: Option<u16>,
    pub frequency_mhz: Option<u32>,
    pub center_frequency_mhz: Option<u32>,
    pub width: Option<ChannelWidth>,
    pub tx_power_dbm: Option<i16>,
    pub busy_percent: Option<f32>,
    pub noise_dbm: Option<i16>,
    /// The foreign networks its node hears on its channel, most disruptive first.
    pub on_channel: Vec<Overlap>,
    /// The sum of their impacts: 0 is a clean channel; 1 is about one strong co-channel network.
    pub interference: f32,
}

/// A network sharing a radio's channel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Overlap {
    pub bssid: MacAddr,
    /// Its primary channel is the radio's.
    pub co_channel: bool,
    /// The share of the radio's channel it covers, from 0 to 1.
    pub overlap: f32,
    /// How strongly the radio's node hears it.
    pub signal_dbm: i16,
    /// `weight(signal) × overlap`, from 0 to 1.
    pub impact: f32,
}

/// One 20 MHz channel of a band.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ChannelReport {
    pub channel: u16,
    pub frequency_mhz: u32,
    /// Some radio of the site may use it (its phy and the regulatory domain allow it).
    pub allowed: bool,
    pub dfs: bool,
    /// The worst state any of the site's radios has for it: radar seen (`unavailable`), else
    /// not checked yet (`usable`), else checked (`available`).
    pub dfs_state: Option<DfsState>,
    /// Radar hits on it in the history given (30 days), and the last.
    pub radar_hits: u32,
    pub last_radar: Option<Timestamp>,
    /// Foreign networks covering any of it, and the strongest of them.
    pub networks: u32,
    pub strongest_dbm: Option<i16>,
    /// The sum of those networks' `weight(signal) × overlap`.
    pub pressure: f32,
    /// How busy it was: live on a channel a radio operates on, else from the latest survey.
    pub busy_percent: Option<f32>,
    pub noise_dbm: Option<i16>,
    /// The site's radios whose channel covers it.
    pub ours: Vec<ChannelUse>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ChannelUse {
    pub node: NodeId,
    pub radio: String,
    /// It is the radio's primary channel.
    pub primary: bool,
}

/// A network heard on the band.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Neighbor {
    /// As the node hearing it best heard it.
    pub bss: Bss,
    pub kind: NeighborKind,
    /// The radio it belongs to, when it is the site's own.
    pub node: Option<NodeId>,
    pub radio: Option<String>,
    /// From the BSSID's OUI; locally administered addresses have none.
    pub vendor: Option<String>,
    /// Which radios heard it, strongest first.
    pub heard_by: Vec<Hearing>,
    /// Its impact summed over the site's radios (foreign networks only).
    pub impact: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum NeighborKind {
    /// One of the adopted nodes' own access points or mesh points.
    Own,
    /// Not adopted, but named like the site's networks (an SSID or mesh ID): an access point
    /// waiting for adoption, or one pretending to be the site's.
    Lookalike,
    Foreign,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Hearing {
    pub node: NodeId,
    pub radio: String,
    pub signal_dbm: i16,
    pub at: Timestamp,
}

/// How node `from` hears node `to` on the band.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct MatrixEntry {
    pub from: NodeId,
    pub to: NodeId,
    pub signal_dbm: i16,
    pub source: MatrixSource,
    pub at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum MatrixSource {
    /// A live mesh peer link (the station dump's signal).
    Mesh,
    /// The latest scan heard the node's access point or mesh point.
    Scan,
}

/// What the API and the WS topic `rf` send: the report, where each radio's scanning stands and
/// the settings.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct RfView {
    pub report: RfReport,
    pub radios: Vec<uniwrt_model::rf::RadioScan>,
    pub settings: uniwrt_model::rf::RfSettings,
}
