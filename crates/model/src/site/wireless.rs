//! Wi-Fi: WLANs (features/wifi.md, WLANs), the mesh backhaul (features/mesh.md) and the site's
//! radio policy, which nodes override per radio ([`super::node::RadioOverride`]).

use serde::{Deserialize, Serialize};
use uniwrt_core::NodeId;

use super::SecretRef;
use crate::wifi::{Band, ChannelWidth};

/// One SSID, broadcast by every radio of its bands on the nodes in scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Wlan {
    /// Its key, which names its UCI sections (`uw_<id>_<band>`): `[a-z][a-z0-9_]{0,11}`.
    pub id: String,
    /// 1–32 bytes.
    pub ssid: String,
    pub enabled: bool,
    /// The id of the network its clients join.
    pub network: String,
    pub security: WlanSecurity,
    /// Required for the PSK and SAE modes.
    pub passphrase: Option<SecretRef>,
    pub bands: Vec<Band>,
    pub nodes: NodeScope,
    pub hidden: bool,
    /// Clients on it cannot reach each other.
    pub isolation: bool,
}

/// How a WLAN authenticates and encrypts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum WlanSecurity {
    /// WPA2-PSK.
    Wpa2,
    /// WPA2/WPA3 transition (`sae-mixed`).
    Wpa2Wpa3,
    /// WPA3-SAE: required on 6 GHz.
    Wpa3,
    /// Opportunistic Wireless Encryption: open, but encrypted.
    Owe,
    /// Open and unencrypted.
    Open,
}

impl WlanSecurity {
    /// Whether the mode needs a passphrase.
    pub const fn needs_passphrase(self) -> bool {
        matches!(self, Self::Wpa2 | Self::Wpa2Wpa3 | Self::Wpa3)
    }
}

/// Which nodes something applies to.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "scope", content = "nodes", rename_all = "snake_case")]
pub enum NodeScope {
    #[default]
    All,
    Only(Vec<NodeId>),
    Except(Vec<NodeId>),
}

/// The mesh backhaul: batman-adv over 802.11s and wired uplinks (D-013).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct MeshConfig {
    pub enabled: bool,
    /// The 802.11s mesh ID, 1–32 bytes.
    pub mesh_id: String,
    /// The SAE key of the mesh, which the controller generates.
    pub key: Option<SecretRef>,
    /// The band the mesh points use; `None` picks per node (features/mesh.md).
    pub band: Option<Band>,
    pub routing: MeshRouting,
}

impl Default for MeshConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mesh_id: "uniwrt-mesh".to_owned(),
            key: None,
            band: None,
            routing: MeshRouting::BatmanIv,
        }
    }
}

/// batman-adv's routing algorithm (O-3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum MeshRouting {
    BatmanIv,
    BatmanV,
}

/// The radios' defaults per band.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RadioPolicy {
    pub bands: Vec<BandPolicy>,
}

impl Default for RadioPolicy {
    fn default() -> Self {
        let band = |band, width| BandPolicy {
            band,
            channel: Channel::Auto,
            width,
            tx_power: TxPower::Auto,
        };
        Self {
            bands: vec![
                band(Band::G2, ChannelWidth::Mhz20),
                band(Band::G5, ChannelWidth::Mhz80),
                band(Band::G6, ChannelWidth::Mhz160),
            ],
        }
    }
}

/// How the radios of one band are set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct BandPolicy {
    pub band: Band,
    pub channel: Channel,
    pub width: ChannelWidth,
    pub tx_power: TxPower,
}

/// A radio's channel: chosen by the node (later the RF planner, M6.1), or fixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    Auto,
    Fixed(u16),
}

/// A radio's transmit power: the regulatory maximum, or a cap in dBm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum TxPower {
    Auto,
    Dbm(u8),
}
