//! The inventory record of an adopted node: identity, management address, role and mode, plus
//! the host key pinned for it.

use serde::{Deserialize, Serialize};
use uniwrt_core::{MacAddr, NodeId, Timestamp};

/// An adopted node as the controller records it. Its identity is the label (or primary) MAC
/// plus the board name, so it survives address changes (features/adoption.md).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Node {
    pub id: NodeId,
    /// Display name; the hostname until the user renames the node.
    pub name: String,
    pub mac: MacAddr,
    pub board_name: String,
    /// Management address: an IP address or a hostname.
    pub host: String,
    /// SSH port.
    pub port: u16,
    pub role: NodeRole,
    pub mode: NodeMode,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

/// A node about to be adopted. There is no mode: every node starts in [`NodeMode::Observe`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct NewNode {
    pub name: String,
    pub mac: MacAddr,
    pub board_name: String,
    pub host: String,
    pub port: u16,
    pub role: NodeRole,
}

/// What a node does in the network (features/adoption.md, Roles).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum NodeRole {
    /// The primary router: WAN, routing, firewall, DHCP and DNS.
    Gateway,
    /// An access point that bridges networks to the gateway.
    Ap,
    /// A wireless client bridge for wired devices.
    Bridge,
}

/// How far the controller may act on a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum NodeMode {
    /// Read-only: the controller only watches. Adopting a node changes nothing but
    /// `authorized_keys` (directive 8).
    Observe,
    /// The controller owns the node's managed packages and applies changes as transactions.
    Managed,
}

/// The SSH host key a node must present, pinned when the user confirmed its fingerprint at
/// adoption (trust on first use). The `node` crate parses and checks it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct HostKeyPin {
    /// OpenSSH form without a comment, such as `ssh-ed25519 AAAAC3Nz…`.
    pub public_key: String,
    /// SHA-256 fingerprint as OpenSSH shows it, such as `SHA256:b4fj9Kwr…`.
    pub fingerprint: String,
    pub pinned_at: Timestamp,
}

#[cfg(test)]
mod tests {
    use serde_json::{json, to_value};

    use super::{NodeMode, NodeRole};

    #[test]
    fn serde_names() {
        assert_eq!(
            to_value([NodeRole::Gateway, NodeRole::Ap, NodeRole::Bridge]).unwrap(),
            json!(["gateway", "ap", "bridge"])
        );
        assert_eq!(
            to_value([NodeMode::Observe, NodeMode::Managed]).unwrap(),
            json!(["observe", "managed"])
        );
    }
}
