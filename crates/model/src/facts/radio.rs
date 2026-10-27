//! Radios (UCI `wifi-device`, kernel phy), what their hardware supports, and their interfaces.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use uniwrt_core::MacAddr;

use crate::wifi::{Band, ChannelWidth, WifiStandard};

/// One radio: its current operating state, its hardware limits and its interfaces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RadioFacts {
    /// UCI section, such as `radio0`.
    pub name: String,
    /// Kernel phy, such as `phy0`.
    pub phy: Option<String>,
    pub band: Band,
    pub up: bool,
    /// Disabled in the configuration.
    pub disabled: bool,
    /// The operating channel; unknown while the radio is down.
    pub channel: Option<u16>,
    pub frequency_mhz: Option<u32>,
    pub width: Option<ChannelWidth>,
    pub standard: Option<WifiStandard>,
    pub tx_power_dbm: Option<i16>,
    pub noise_dbm: Option<i16>,
    /// Share of time the channel was busy (any transmitter) since the previous collection.
    pub channel_busy_percent: Option<f32>,
    /// Regulatory country code, such as `US`.
    pub country: Option<String>,
    pub capabilities: RadioCapabilities,
    pub ifaces: Vec<WifiIfaceFacts>,
}

/// What the radio hardware and the regulatory domain allow.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RadioCapabilities {
    pub bands: BTreeSet<Band>,
    pub standards: BTreeSet<WifiStandard>,
    pub widths: BTreeSet<ChannelWidth>,
    /// Usable channels; channels the regulatory domain disables are left out.
    pub channels: Vec<Channel>,
    pub max_tx_power_dbm: Option<i16>,
    pub spatial_streams: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Channel {
    pub band: Band,
    pub number: u16,
    pub frequency_mhz: u32,
    /// Radar detection required before use.
    pub dfs: bool,
    /// No initiating radiation: the radio may not start a BSS on this channel.
    pub no_ir: bool,
    pub max_tx_power_dbm: Option<i16>,
}

/// A Wi-Fi interface (UCI `wifi-iface`): an AP's BSS, a station uplink or a mesh point.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WifiIfaceFacts {
    /// Kernel device, such as `phy1-ap0`.
    pub name: String,
    /// UCI section, such as `default_radio1`.
    pub section: Option<String>,
    pub mode: WifiIfaceMode,
    pub ssid: Option<String>,
    pub mesh_id: Option<String>,
    pub bssid: Option<MacAddr>,
    /// UCI `encryption` value, such as `sae-mixed` or `psk2+ccmp`.
    pub encryption: Option<String>,
    /// The UCI networks the interface is attached to.
    pub networks: Vec<String>,
    pub up: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WifiIfaceMode {
    Ap,
    Sta,
    Mesh,
    Adhoc,
    Monitor,
}
