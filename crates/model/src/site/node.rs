//! Per-node desired state: which UCI packages the controller owns on the node (D-011), its
//! radios' overrides, and raw UCI sections (the validated escape hatch, also where brownfield
//! import keeps what it cannot map). The node's role stays on its inventory record.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use uniwrt_core::NodeId;

use super::wireless::{Channel, TxPower};
use crate::wifi::ChannelWidth;

/// One node's overrides.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct NodeConfig {
    pub node: NodeId,
    /// The hostname to set; `None` keeps the node's own.
    pub hostname: Option<String>,
    /// The packages the controller renders and owns whole; the others it never touches.
    pub managed: BTreeSet<ManagedPackage>,
    pub radios: Vec<RadioOverride>,
    pub raw: Vec<RawSection>,
}

impl NodeConfig {
    /// A node with no overrides and nothing managed.
    pub fn new(node: NodeId) -> Self {
        Self {
            node,
            hostname: None,
            managed: BTreeSet::new(),
            radios: Vec::new(),
            raw: Vec::new(),
        }
    }
}

/// A UCI package the controller can own (D-011); the set grows feature by feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ManagedPackage {
    System,
    Network,
    Wireless,
    Dhcp,
    Firewall,
    Dropbear,
    Usteer,
    Sqm,
    Wireguard,
}

impl ManagedPackage {
    /// The UCI package's name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Network => "network",
            Self::Wireless => "wireless",
            Self::Dhcp => "dhcp",
            Self::Firewall => "firewall",
            Self::Dropbear => "dropbear",
            Self::Usteer => "usteer",
            Self::Sqm => "sqm",
            Self::Wireguard => "wireguard",
        }
    }
}

/// How one radio differs from the site's band policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RadioOverride {
    /// The radio's UCI `wifi-device` name, such as `radio0`.
    pub radio: String,
    pub disabled: bool,
    pub channel: Option<Channel>,
    pub width: Option<ChannelWidth>,
    pub tx_power: Option<TxPower>,
}

/// A raw UCI section, added to (or replacing the same named section of) what is rendered for
/// its package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RawSection {
    /// The UCI package, such as `network`.
    pub package: String,
    /// The section's type, such as `interface`.
    #[serde(rename = "type")]
    pub kind: String,
    /// The section's name; `None` is an anonymous section.
    pub name: Option<String>,
    pub options: BTreeMap<String, RawValue>,
}

/// A UCI option's value: one string (`option`) or several (`list`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(untagged)]
pub enum RawValue {
    Option(String),
    List(Vec<String>),
}
