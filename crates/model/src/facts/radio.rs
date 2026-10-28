//! Radios (UCI `wifi-device`) and their interfaces: what they are configured to do and how they
//! operate. Their hardware is a [`PhyFacts`](super::PhyFacts), named by [`RadioFacts::phy`].

use serde::{Deserialize, Serialize};
use uniwrt_core::MacAddr;

use crate::wifi::{Band, ChannelWidth, WifiStandard};

/// One radio: its configuration, its current operating state and its interfaces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RadioFacts {
    /// UCI section, such as `radio0`.
    pub name: String,
    /// Kernel phy, such as `phy0`; unknown while a radio without interfaces is down.
    pub phy: Option<String>,
    pub band: Band,
    pub up: bool,
    /// Disabled in the configuration.
    pub disabled: bool,
    /// The operating channel; unknown while the radio is down.
    pub channel: Option<u16>,
    pub frequency_mhz: Option<u32>,
    /// The center of the whole channel, which differs from `frequency_mhz` above 20 MHz.
    pub center_frequency_mhz: Option<u32>,
    pub width: Option<ChannelWidth>,
    /// The configured standard (the PHY of UCI `htmode`).
    pub standard: Option<WifiStandard>,
    pub tx_power_dbm: Option<i16>,
    pub noise_dbm: Option<i16>,
    /// Share of time the channel was busy (any transmitter) since the previous collection.
    pub channel_busy_percent: Option<f32>,
    /// Regulatory country code, such as `US`.
    pub country: Option<String>,
    pub ifaces: Vec<WifiIfaceFacts>,
}

/// A Wi-Fi interface (UCI `wifi-iface`): an AP's BSS, a station uplink or a mesh point.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WifiIfaceFacts {
    /// Kernel device, such as `phy1-ap0`; unknown while it is down.
    pub name: Option<String>,
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
