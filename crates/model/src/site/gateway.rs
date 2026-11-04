//! What only the gateway renders: the WAN (features/wan.md), the firewall policy between
//! networks (features/network.md, Firewall policy) and services such as local DNS. Schema v1
//! has their basics; M4 and M8 extend them.

use std::net::{IpAddr, Ipv4Addr};

use serde::{Deserialize, Serialize};
use uniwrt_core::{Ipv4Net, MacAddr};

use super::SecretRef;

/// The gateway's uplink.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct WanConfig {
    pub proto: WanProto,
    /// The VLAN the ISP tags the uplink with, such as 201.
    pub vlan: Option<u16>,
    /// A MAC address to clone onto the WAN device.
    pub mac: Option<MacAddr>,
    /// Ask for IPv6 (DHCPv6 with prefix delegation).
    pub ipv6: bool,
}

/// How the WAN gets its address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WanProto {
    Dhcp,
    Static {
        #[cfg_attr(feature = "ts", ts(as = "String"))]
        address: Ipv4Net,
        #[cfg_attr(feature = "ts", ts(as = "String"))]
        gateway: Ipv4Addr,
        dns: Vec<IpAddr>,
    },
    Pppoe {
        username: String,
        password: Option<SecretRef>,
    },
}

/// The firewall policy between networks. Each network's purpose seeds a matrix; these entries
/// override cells of it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FirewallConfig {
    pub policies: Vec<ZonePolicy>,
}

/// One cell of the matrix: traffic from one network to another (or `wan`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ZonePolicy {
    /// A network id, or `wan`.
    pub from: String,
    /// A network id, or `wan`.
    pub to: String,
    pub action: ZoneAction,
}

/// What a [`ZonePolicy`] does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ZoneAction {
    Allow,
    Block,
    /// Only replies to connections from the other side.
    Established,
}

/// Services on the gateway.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Services {
    /// Custom local DNS records, besides the automatic `<hostname>.<domain>`.
    pub dns_records: Vec<DnsRecord>,
}

/// A local DNS record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DnsRecord {
    /// An A or AAAA record.
    Host {
        name: String,
        #[cfg_attr(feature = "ts", ts(as = "String"))]
        ip: IpAddr,
    },
    Cname {
        name: String,
        target: String,
    },
}
