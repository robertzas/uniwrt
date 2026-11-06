//! Networks (features/network.md, Networks): a VLAN with its subnet, DHCP and IPv6 mode, which
//! every node carries and the gateway routes.

use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};
use uniwrt_core::{Ipv4Net, MacAddr};

/// One network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Network {
    /// Its key, which names its UCI interface, firewall zone and DHCP pool:
    /// `[a-z][a-z0-9_]{0,11}`.
    pub id: String,
    /// Display name.
    pub name: String,
    pub purpose: Purpose,
    /// The 802.1Q VLAN ID (1–4094); `None` is untagged.
    pub vlan: Option<u16>,
    /// The gateway's address and DHCP; `None` leaves the network without IPv4 on the gateway.
    pub ipv4: Option<Ipv4Config>,
    pub ipv6: Ipv6Mode,
    /// How clients get their IPv6 addresses, when the network has IPv6.
    pub ipv6_clients: Ipv6Clients,
    /// Clients on it cannot reach each other.
    pub isolation: bool,
    /// mDNS is reflected between this network and the others that have it on.
    pub mdns: bool,
    pub igmp_snooping: bool,
}

/// What a network is for, which seeds its firewall policy (features/network.md, Firewall).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    /// The main LAN: reaches everything.
    Default,
    /// Smart-home devices: the internet, not the LAN.
    Iot,
    /// Visitors: the internet only, isolated.
    Guest,
    /// The nodes' and controller's own network.
    Mgmt,
    /// No seeded policy beyond the internet.
    Custom,
}

/// The gateway's IPv4 side of a network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Ipv4Config {
    /// The gateway's address with the subnet's prefix length, such as `10.0.30.1/24`.
    #[cfg_attr(feature = "ts", ts(as = "String"))]
    pub address: Ipv4Net,
    /// The DHCP server; `None` leaves addressing to static configuration.
    pub dhcp: Option<Dhcp>,
}

/// A DHCPv4 pool and its reservations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Dhcp {
    #[cfg_attr(feature = "ts", ts(as = "String"))]
    pub start: Ipv4Addr,
    /// The last address of the pool, inclusive.
    #[cfg_attr(feature = "ts", ts(as = "String"))]
    pub end: Ipv4Addr,
    pub lease_time_s: u32,
    pub reservations: Vec<Reservation>,
    /// What the pool hands out besides addresses.
    pub options: DhcpOptions,
}

/// The DHCP options a pool hands out (features/network.md, DHCP & local DNS).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct DhcpOptions {
    /// DNS servers (option 6); empty hands out the gateway, which resolves local names.
    #[cfg_attr(feature = "ts", ts(as = "Vec<String>"))]
    pub dns: Vec<Ipv4Addr>,
    /// NTP servers (option 42); empty hands out none.
    #[cfg_attr(feature = "ts", ts(as = "Vec<String>"))]
    pub ntp: Vec<Ipv4Addr>,
    /// The domain (option 15); `None` hands out the site's.
    pub domain: Option<String>,
    /// Network boot (PXE).
    pub boot: Option<Boot>,
    /// Other options as dnsmasq reads them: `<code>,<value>` or `option:<name>,<value>`.
    pub custom: Vec<String>,
}

/// Where a network-booting client finds its boot file (dnsmasq's `dhcp-boot`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Boot {
    /// The boot file, such as `pxelinux.0` or `netboot.xyz.efi`.
    pub filename: String,
    /// The TFTP server's name.
    pub server_name: Option<String>,
    /// The TFTP server's address; `None` is the gateway.
    #[cfg_attr(feature = "ts", ts(as = "Option<String>"))]
    pub server: Option<Ipv4Addr>,
}

/// A fixed address for a client ("Fix IP").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Reservation {
    pub mac: MacAddr,
    #[cfg_attr(feature = "ts", ts(as = "String"))]
    pub ip: Ipv4Addr,
    /// The hostname handed out, if any.
    pub name: Option<String>,
}

/// How a network gets IPv6 (features/network.md, IPv6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Ipv6Mode {
    Off,
    /// A /64 of the site's ULA.
    Ula,
    /// A /64 of the prefix the WAN delegates.
    Delegated,
    /// Both.
    Both,
}

impl Ipv6Mode {
    /// Whether the network has IPv6.
    pub fn is_on(self) -> bool {
        self != Self::Off
    }
}

/// How clients get IPv6 addresses (odhcpd's router advertisements and DHCPv6).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Ipv6Clients {
    /// SLAAC only: router advertisements, no DHCPv6.
    Slaac,
    /// SLAAC, with stateless DHCPv6 for DNS and other options.
    #[default]
    SlaacStateless,
    /// SLAAC and stateful DHCPv6 addresses side by side, as OpenWrt ships.
    SlaacStateful,
    /// Stateful DHCPv6 only: Android, which has no DHCPv6 client, gets no IPv6 address.
    Stateful,
}
