//! Discovery (features/adoption.md, Discovery): devices that the adopted nodes see, which look
//! like OpenWrt nodes and are not adopted yet, for one-click or batch adoption.

use std::net::IpAddr;

use serde::{Deserialize, Serialize};
use uniwrt_core::{MacAddr, NodeId, Timestamp};

/// What discovery found, as it stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct DiscoveryReport {
    /// By address, then MAC address.
    pub nodes: Vec<Discovered>,
    /// Probes are still running: asking again soon may find more.
    pub scanning: bool,
}

/// A device that looks like an OpenWrt node and is not adopted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Discovered {
    /// For a mesh node without a known address, its batman-adv originator.
    pub mac: MacAddr,
    /// Where to adopt it; `None` for a mesh node whose address is unknown.
    pub ip: Option<IpAddr>,
    /// From its DHCP lease.
    pub hostname: Option<String>,
    /// How the adopted nodes know it, in this order.
    pub sources: Vec<DiscoverySource>,
    /// The adopted node that knows it first (the gateway, for its leases).
    pub seen_by: NodeId,
    pub probe: ProbeState,
    /// The SSH server's version line, such as `SSH-2.0-dropbear_2024.86`.
    pub ssh: Option<String>,
    /// The host key it presented, for the user to confirm before adopting it.
    pub host_key: Option<PresentedKey>,
    pub probed_at: Option<Timestamp>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum DiscoverySource {
    /// A DHCP lease the node handed out.
    Lease,
    /// The node's IPv4 neighbor cache, on a non-WAN interface.
    Neighbor,
    /// batman-adv: a mesh originator, or a wired device another originator announces.
    Mesh,
}

/// How far probing its SSH port got.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ProbeState {
    /// Queued or running.
    Probing,
    /// Port 22 answered; `ssh` says whether with SSH.
    Done,
    /// Port 22 refused or did not answer.
    Unreachable,
    /// Not probed: no address, or this controller reaches no nodes (demo mode).
    Skipped,
}

/// A host key as a probe read it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PresentedKey {
    /// OpenSSH form, which `POST /api/v1/adopt` takes once the user confirmed it.
    pub host_key: String,
    /// SHA-256, as OpenSSH shows it: `SHA256:…`.
    pub fingerprint: String,
    pub algorithm: String,
}
