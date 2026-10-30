//! batman-adv meshes, as batctl reports them: the node's view of the other nodes (originators),
//! its direct neighbors, the gateways, and which node each client sits behind (the translation
//! tables).

use serde::{Deserialize, Serialize};
use uniwrt_core::MacAddr;

/// One mesh device of the node, such as `bat0`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct BatmanFacts {
    /// The mesh device, such as `bat0`.
    pub mesh_if: String,
    pub mac: Option<MacAddr>,
    pub algorithm: Option<BatmanAlgorithm>,
    pub gw_mode: Option<GatewayMode>,
    /// The interfaces the mesh runs over (802.11s mesh points, Ethernet ports).
    pub hard_ifs: Vec<HardIf>,
    /// Every other node the mesh knows, with each route to it.
    pub originators: Vec<Originator>,
    /// The nodes heard directly, per hard interface.
    pub neighbors: Vec<BatmanNeighbor>,
    /// The nodes that announce themselves as gateways.
    pub gateways: Vec<BatmanGateway>,
    /// Clients that other nodes announce (the global translation table): where each one sits.
    pub remote_clients: Vec<RemoteClient>,
    /// Clients this node announces (the local translation table), without its own interfaces.
    pub local_clients: Vec<LocalClient>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum BatmanAlgorithm {
    #[serde(rename = "BATMAN_IV")]
    BatmanIv,
    #[serde(rename = "BATMAN_V")]
    BatmanV,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum GatewayMode {
    Off,
    Client,
    Server,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct HardIf {
    pub name: String,
    pub mac: Option<MacAddr>,
    /// Up and carrying the mesh.
    pub active: bool,
}

/// A route to another node: through which neighbor, and how good it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Originator {
    pub mac: MacAddr,
    pub next_hop: MacAddr,
    pub hard_if: String,
    pub last_seen_ms: u64,
    /// Transmit quality, 0–255 (`BATMAN_IV`).
    pub tq: Option<u8>,
    /// Estimated throughput (`BATMAN_V`).
    pub throughput_kbps: Option<u32>,
    /// The route the mesh uses.
    pub best: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct BatmanNeighbor {
    pub mac: MacAddr,
    pub hard_if: String,
    pub last_seen_ms: u64,
    /// Estimated throughput (`BATMAN_V`).
    pub throughput_kbps: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct BatmanGateway {
    /// The gateway node.
    pub originator: MacAddr,
    pub next_hop: MacAddr,
    pub hard_if: String,
    pub tq: Option<u8>,
    pub throughput_kbps: Option<u32>,
    /// The bandwidth the gateway advertises.
    pub down_kbps: Option<u32>,
    pub up_kbps: Option<u32>,
    /// The gateway this node uses (in client mode).
    pub best: bool,
}

/// A client behind another node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RemoteClient {
    pub mac: MacAddr,
    /// The VLAN it was announced on; `None` when untagged.
    pub vid: Option<u16>,
    /// The node it sits behind.
    pub originator: MacAddr,
    /// Connected over Wi-Fi.
    pub wifi: bool,
    /// Roaming to another node.
    pub roaming: bool,
}

/// A client behind this node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LocalClient {
    pub mac: MacAddr,
    pub vid: Option<u16>,
    pub wifi: bool,
    pub last_seen_ms: Option<u64>,
}
