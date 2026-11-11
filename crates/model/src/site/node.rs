//! Per-node desired state: which UCI packages the controller owns on the node (D-011), its
//! radios' overrides, and raw UCI sections (the validated escape hatch, also where brownfield
//! import keeps what it cannot map). The node's role stays on its inventory record.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use uniwrt_core::NodeId;

use super::{
    SecretRef,
    wireless::{Channel, TxPower},
};
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
    /// The Ethernet ports that don't carry every network; the others are trunks.
    pub ports: Vec<PortConfig>,
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
            ports: Vec::new(),
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
    Mwan3,
    Upnpd,
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
            Self::Mwan3 => "mwan3",
            Self::Upnpd => "upnpd",
            Self::Wireguard => "wireguard",
        }
    }
}

/// Which networks an Ethernet port carries (features/network.md, Networks). A port without
/// one is a trunk: the untagged network untagged, every network with a VLAN tagged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PortConfig {
    /// The port: a network device such as `lan2` or `eth1`, or a swconfig switch's port by
    /// number, such as `switch0:3`.
    pub port: String,
    /// The network it carries untagged, which untagged frames join; `None` carries none.
    pub native: Option<String>,
    pub tagged: TaggedNetworks,
}

impl PortConfig {
    /// The swconfig switch port's name.
    pub fn switch_port(switch: &str, number: u8) -> String {
        format!("{switch}:{number}")
    }
}

/// The networks a port carries tagged: only networks with a VLAN, never its native one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaggedNetworks {
    /// Every network with a VLAN, including ones added later.
    All,
    None,
    /// These networks, by id.
    Only {
        networks: Vec<String>,
    },
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

/// A raw UCI section: added to (or replacing the same named section of) what is rendered for
/// its package, or with `merge` its options set on a rendered section.
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
    /// Set the options on the rendered section instead: the one of this name, or for an
    /// anonymous section the one of its type that shares its key options (render's
    /// `raw::key`, such as a `zone`'s `name`). Brownfield import keeps a node's options that
    /// render doesn't write this way, so the site's edits still reach the section.
    #[serde(default)]
    pub merge: bool,
}

/// A UCI option's value: one string (`option`), several (`list`), or a secret sealed in the
/// vault (`{"secret": "config/…"}`), which render writes as one string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(untagged)]
pub enum RawValue {
    Option(String),
    List(Vec<String>),
    /// The value of an option that holds a secret ([`is_secret_option`]), such as a WireGuard
    /// `private_key`: brownfield import seals these (D-073).
    Secret {
        secret: SecretRef,
    },
}

/// The UCI options that hold secrets in the packages render writes: Wi-Fi keys and RADIUS
/// secrets, passwords of WAN protocols, WireGuard's keys. Rendered packages carry their values,
/// so review screens mask them; raw sections hold them sealed ([`RawValue::Secret`]).
pub const SECRET_OPTIONS: [&str; 15] = [
    "key",
    "key1",
    "key2",
    "key3",
    "key4",
    "password",
    "sae_password",
    "auth_secret",
    "acct_secret",
    "dae_secret",
    "priv_key_pwd",
    "private_key",
    "preshared_key",
    // 802.11r key holders: each entry ends with the key their frames are protected with.
    "r0kh",
    "r1kh",
];

/// Whether a UCI option of that name holds a secret ([`SECRET_OPTIONS`]).
pub fn is_secret_option(name: &str) -> bool {
    SECRET_OPTIONS.contains(&name)
}
