//! The topology graph: what [`build`](crate::build) returns, the API serves and the WS topic
//! `topology` keeps current.

use std::net::IpAddr;

use serde::{Deserialize, Serialize};
use uniwrt_core::{MacAddr, NodeId, Timestamp};
use uniwrt_model::{status::NodeState, wifi::Band};

/// The network as a graph. Nodes and edges are sorted by id, and ids are stable from one build
/// to the next, so two builds compare by id ([`Topology::diff`]).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Topology {
    /// When the newest facts it was built from were collected, on the controller's clock.
    pub at: Timestamp,
    pub nodes: Vec<TopoNode>,
    pub edges: Vec<TopoEdge>,
}

/// One vertex: the internet, an adopted node, an inferred switch, a mesh peer that is not
/// adopted, or a client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct TopoNode {
    /// `internet`, `node:<id>`, `switch:<node id>:<port>` (the first port it was seen on),
    /// `peer:<mac>` or `client:<mac>`.
    pub id: String,
    pub kind: TopoKind,
    /// The adopted node it is.
    pub node: Option<NodeId>,
    /// A client's or peer's MAC address; an adopted node's adoption MAC.
    pub mac: Option<MacAddr>,
    /// A client's DHCP hostname; an adopted node's hostname.
    pub name: Option<String>,
    /// A client's addresses (its leases and the neighbor caches); an adopted node's own.
    pub ips: Vec<IpAddr>,
    /// An adopted node's status.
    pub state: Option<NodeState>,
    pub cpu_percent: Option<f32>,
    /// The share of memory in use (not available).
    pub memory_percent: Option<f32>,
    /// The vertex it hangs from in the tree view, toward the internet (or the gateway), and the
    /// edge to it; `None` for the roots and for anything not connected to them.
    pub parent: Option<String>,
    pub uplink: Option<String>,
    /// Its depth in the tree view: 0 for a root (the internet, or a gateway that does not reach
    /// it).
    pub hops: Option<u16>,
    /// The links from it to its gateway along the tree view, a cable through an unmanaged switch
    /// counting once; `None` for clients and for what no gateway's tree holds.
    pub gateway_hops: Option<u16>,
    /// Of those, the links over the air (a mesh link, or a station's link to its access point).
    pub wireless_hops: Option<u16>,
    /// The nearest node, switch or peer (never a gateway) that each of its paths to a gateway
    /// crosses, over links that are up: what it can't lose. `None` where nothing but the
    /// gateway is, and where no gateway is reached.
    pub depends_on: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum TopoKind {
    Internet,
    /// Adopted nodes, by role.
    Gateway,
    Ap,
    Bridge,
    /// An unmanaged switch, inferred where several nodes or clients share one port.
    Switch,
    /// A node of the mesh that is not adopted (a batman-adv or 802.11s neighbor).
    Peer,
    Client,
}

/// A link between two vertices. One edge stands for one link, whichever of its ends reported
/// it (and however many did).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct TopoEdge {
    /// Both ends' ids in byte order, the medium, and what tells parallel links apart (the band,
    /// or the WAN device), joined by `~`.
    pub id: String,
    /// The upstream end, toward the internet or the gateway, when the tree says; else the end
    /// with fewer hops.
    pub a: String,
    pub b: String,
    pub medium: Medium,
    /// The port or interface at each end, such as `lan2`, `phy2-mesh0`, `bh0` or `eth1`.
    pub a_port: Option<String>,
    pub b_port: Option<String>,
    pub up: bool,
    pub band: Option<Band>,
    pub ssid: Option<String>,
    /// A station's signal as its access point hears it; for a mesh link, the weaker end's.
    pub signal_dbm: Option<i16>,
    /// batman-adv's transmit quality (0–255), the lower of both ends'.
    pub tq: Option<u8>,
    /// What the link can carry: the negotiated Ethernet speed, the driver's expected
    /// throughput (or the PHY rate) of a station, or batman-adv's throughput estimate.
    pub capacity_mbps: Option<u32>,
    /// 0–100, from the signal or the TQ; `None` where it means nothing (Ethernet).
    pub quality: Option<u8>,
    pub traffic: Option<Traffic>,
    /// The health of a link between nodes, switches or peers, 0–100: the lowest of its
    /// factors ([`LinkFactor`]); `None` for the WAN and clients' links.
    pub score: Option<u8>,
    /// The factor that sets the score, when it is below 100.
    pub weakest: Option<LinkFactor>,
    /// When it went down or vanished while both its ends were online, in the last
    /// [`DROP_WINDOW_MS`](crate::DROP_WINDOW_MS), oldest first; kept by
    /// [`LinkWatch`](crate::LinkWatch). Builds stored before it have none (as for
    /// `critical`).
    #[serde(default)]
    pub drops: Vec<Timestamp>,
    /// Whether a node, switch or peer reaches the gateway only over this link (of the links
    /// that are up).
    #[serde(default)]
    pub critical: bool,
}

/// What a link's score comes from; each gives 0–100 and the lowest is the score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum LinkFactor {
    /// Down: 0.
    Down,
    /// Two drops or more in the window: 30 points off for each after the first.
    Flapping,
    /// The weaker end's signal, as [`TopoEdge::quality`] scales it.
    Signal,
    /// batman-adv's transmit quality.
    Tq,
    /// What the link can carry, on a log scale from 10 Mbit/s (0) to 300 (100); a cable only
    /// below 100 Mbit/s.
    Throughput,
}

/// How a link is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Medium {
    /// The gateway's uplink to the internet.
    Wan,
    Ethernet,
    /// batman-adv or 802.11s between nodes.
    Mesh,
    /// A station and its access point.
    Wifi,
}

/// The bytes a link carried, `down` from `a` to `b` and `up` from `b` to `a`, as one end's
/// counters say.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Traffic {
    /// When the counters were read.
    pub at: Timestamp,
    pub down_bytes: u64,
    pub up_bytes: u64,
    /// Since the previous build's reading; `None` for the first one, or once the counters
    /// restarted.
    pub down_bps: Option<u64>,
    pub up_bps: Option<u64>,
}
