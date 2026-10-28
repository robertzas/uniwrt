//! Who is next to the node at layers 2 and 3: the ARP and NDP caches, and the MAC addresses the
//! bridges learned on their ports.

use std::net::IpAddr;

use serde::{Deserialize, Serialize};
use uniwrt_core::MacAddr;

/// An entry of the neighbor cache: the MAC address that holds an IP address on a device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NeighborFacts {
    pub ip: IpAddr,
    pub mac: MacAddr,
    pub device: String,
    /// Unknown when the source has no states (/proc/net/arp).
    pub state: Option<NeighborState>,
    /// An IPv6 router (NDP).
    pub router: bool,
}

/// The kernel's neighbor states (`NUD_*`) that an entry with a MAC address can be in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NeighborState {
    Reachable,
    Stale,
    Delay,
    Probe,
    Permanent,
}

/// A MAC address a bridge learned on one of its ports (an entry of its forwarding database).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FdbEntry {
    pub mac: MacAddr,
    pub bridge: String,
    /// The port it was learned on, such as `lan2` or `bat0`.
    pub port: String,
    pub vlan: Option<u16>,
}
