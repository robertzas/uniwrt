//! Logical interfaces (netifd) and the kernel network devices beneath them.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use serde::{Deserialize, Serialize};
use uniwrt_core::{Ipv4Net, Ipv6Net, MacAddr};

/// A netifd interface (`network.interface` status): the layer-3 view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterfaceFacts {
    /// UCI name, such as `lan` or `wan6`.
    pub name: String,
    /// Protocol handler: `static`, `dhcp`, `dhcpv6`, `pppoe`, `batadv`, `none` and others.
    pub proto: String,
    pub up: bool,
    /// The layer-3 device, such as `br-lan` or `pppoe-wan`.
    pub device: Option<String>,
    /// Time since the interface came up.
    pub uptime_s: Option<u64>,
    /// Addresses with their prefix length (host bits kept).
    pub ipv4: Vec<Ipv4Net>,
    pub ipv6: Vec<Ipv6Net>,
    /// Prefixes delegated by the upstream router (DHCPv6-PD).
    pub delegated_prefixes: Vec<Ipv6Net>,
    pub ipv4_gateway: Option<Ipv4Addr>,
    pub ipv6_gateway: Option<Ipv6Addr>,
    pub dns_servers: Vec<IpAddr>,
    /// netifd error codes, such as `NO_DEVICE` or `AUTH_FAILED`.
    pub errors: Vec<String>,
}

/// A kernel network device (sysfs and `/proc/net/dev`): the layer-2 view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceFacts {
    pub name: String,
    pub kind: DeviceKind,
    /// `None` for layer-3 tunnels.
    pub mac: Option<MacAddr>,
    pub mtu: u32,
    /// Administratively up.
    pub up: bool,
    pub carrier: bool,
    /// Negotiated Ethernet speed.
    pub speed_mbps: Option<u32>,
    pub full_duplex: Option<bool>,
    /// The bridge or batman-adv mesh this device is a port of.
    pub master: Option<String>,
    pub counters: Counters,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    /// A physical port, including DSA user ports.
    Ethernet,
    Bridge,
    /// An 802.1Q VLAN device.
    Vlan,
    /// A Wi-Fi interface (AP, station, mesh or monitor).
    Wireless,
    /// A batman-adv mesh interface (`bat0`).
    Batman,
    /// WireGuard, GRE and other tunnels.
    Tunnel,
    Ppp,
    Loopback,
    #[serde(other)]
    Other,
}

/// Cumulative counters since the device was created.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Counters {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub rx_errors: u64,
    pub tx_errors: u64,
    pub rx_dropped: u64,
    pub tx_dropped: u64,
}
