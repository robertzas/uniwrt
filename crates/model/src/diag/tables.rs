//! A node's neighbor, route and connection tables (D-142), as the controller reads them on
//! demand (`uniwrt_node::diag::tables`): the kernel's neighbor cache (`ip neigh`), every
//! routing table of both families (`ip route show table all`), and connection tracking
//! (`/proc/net/nf_conntrack`, at most [`MAX_CONNECTIONS`] entries).

use std::net::IpAddr;

use serde::{Deserialize, Serialize};
use uniwrt_core::MacAddr;

use super::IpFamily;

/// The most tracked connections read from a node: a busy gateway tracks tens of thousands.
pub const MAX_CONNECTIONS: usize = 4096;

/// An entry of the neighbor cache: ARP for IPv4, NDP for IPv6.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct NeighborEntry {
    pub address: IpAddr,
    /// None while unresolved (`INCOMPLETE`, `FAILED`).
    pub mac: Option<MacAddr>,
    pub device: String,
    /// The kernel's state, such as `REACHABLE`, `STALE`, `DELAY`, `FAILED` or `PERMANENT`.
    pub state: Option<String>,
    /// An IPv6 router.
    pub router: bool,
}

/// A route of any table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Route {
    pub family: IpFamily,
    /// `unicast`, or `local`, `broadcast`, `multicast`, `anycast`, `unreachable`,
    /// `blackhole`, `prohibit`, `throw`.
    pub kind: String,
    /// `default`, a prefix, or an address (a host route).
    pub destination: String,
    /// `main` when the route doesn't name one.
    pub table: String,
    pub protocol: Option<String>,
    pub scope: Option<String>,
    /// The preferred source address.
    pub source: Option<IpAddr>,
    pub metric: Option<u32>,
    /// Where it goes: one hop, or several for a multipath route; none for one that goes nowhere
    /// (`unreachable`).
    pub hops: Vec<NextHop>,
}

/// A route's next hop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct NextHop {
    /// The gateway; none for a directly connected destination.
    pub via: Option<IpAddr>,
    pub device: Option<String>,
    /// A multipath hop's weight.
    pub weight: Option<u32>,
}

/// A tracked connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TrackedConnection {
    pub family: IpFamily,
    /// `tcp`, `udp`, `icmp`, `icmpv6`, `sctp`, `gre`… or `unknown`.
    pub protocol: String,
    /// The IP protocol number.
    pub protocol_number: u8,
    /// TCP's (or SCTP's) state, such as `ESTABLISHED`.
    pub state: Option<String>,
    /// Seconds until it is forgotten without traffic.
    pub expires_s: u32,
    /// As the first packet went.
    pub original: TrackedFlow,
    /// As answers come back (after NAT, the addresses differ).
    pub reply: TrackedFlow,
    /// Seen both ways long enough to be kept under pressure.
    pub assured: bool,
    /// No answer seen yet.
    pub unreplied: bool,
    /// Forwarded by flow offloading (software or hardware), past the firewall's counters.
    pub offloaded: bool,
    pub mark: u32,
}

/// One direction of a tracked connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TrackedFlow {
    pub src: IpAddr,
    pub dst: IpAddr,
    /// Ports of TCP, UDP, SCTP and the like.
    pub sport: Option<u16>,
    pub dport: Option<u16>,
    /// Counted only with `nf_conntrack_acct` on, as OpenWrt sets it.
    pub packets: Option<u64>,
    pub bytes: Option<u64>,
}

/// The connections read from a node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ConntrackTable {
    /// At most [`MAX_CONNECTIONS`], in the kernel's order.
    pub connections: Vec<TrackedConnection>,
    /// How many the kernel tracks.
    pub count: u32,
    /// How many it can track (`nf_conntrack_max`); none when it tracks nothing, without the
    /// conntrack module (a node whose firewall never ran).
    pub max: Option<u32>,
    /// More were tracked than read.
    pub truncated: bool,
}
