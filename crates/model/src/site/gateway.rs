//! What only the gateway renders besides its firewall ([`firewall`](super::firewall)): the WAN
//! (features/wan.md) and services such as local DNS. Schema v1 has their basics; M4 and M8
//! extend them.

use std::net::{IpAddr, Ipv4Addr};

use serde::{Deserialize, Serialize};
use uniwrt_core::{Ipv4Net, Ipv6Net, MacAddr};

use super::SecretRef;

/// The gateway's uplink (features/wan.md). The wizard fills it from an ISP preset; every field
/// stays editable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct WanConfig {
    /// How IPv4 arrives.
    pub proto: WanProto,
    /// The VLAN the ISP tags the uplink with, such as 201.
    pub vlan: Option<u16>,
    /// The 802.1p priority (0 to 7) every frame leaves the VLAN with, for ISPs that require one.
    pub pcp: Option<u8>,
    /// The interface's MTU, where the ISP's differs from the default (1500, PPPoE's 1492).
    pub mtu: Option<u16>,
    /// A MAC address to clone onto the WAN device.
    pub mac: Option<MacAddr>,
    pub ipv6: WanIpv6,
    /// The ISP preset the wizard last filled this from (crates/isp), shown beside the fields.
    pub preset: Option<String>,
    /// Smart queueing on the WAN (cake through sqm-scripts); unset, off.
    pub sqm: Option<Sqm>,
    /// A backup line mwan3 fails over to while this one is down; unset, none.
    pub failover: Option<Failover>,
}

impl Default for WanConfig {
    /// DHCP with DHCPv6 prefix delegation on the board's WAN port: cable and most fiber.
    fn default() -> Self {
        Self {
            proto: WanProto::Dhcp(DhcpClient::default()),
            vlan: None,
            pcp: None,
            mtu: None,
            mac: None,
            ipv6: WanIpv6::Dhcpv6 { reqprefix: None },
            preset: None,
            sqm: None,
            failover: None,
        }
    }
}

/// How the WAN gets its IPv4 address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WanProto {
    Dhcp(DhcpClient),
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
        /// The access concentrator's service name, for ISPs that require one.
        service: Option<String>,
    },
    /// DS-Lite: IPv4 in an IPv6 tunnel to the ISP's AFTR, over the DHCPv6 the IPv6 settings
    /// bring.
    DsLite {
        /// The AFTR's name or address; unset, DHCPv6 announces it.
        aftr: Option<String>,
    },
    /// No IPv4 on the WAN: IPv6 only.
    None,
}

/// What the DHCP client sends, for ISPs that check it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct DhcpClient {
    /// Option 60.
    pub vendor_class: Option<String>,
    /// Option 61, in hexadecimal.
    pub client_id: Option<String>,
    /// Further options, each `<code>:<value>` as udhcpc takes them, such as `77:4d4f44454d`.
    pub send_options: Vec<String>,
    /// The hostname sent in option 12; unset, the gateway's own.
    pub hostname: Option<String>,
}

/// How the WAN gets IPv6.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum WanIpv6 {
    Off,
    /// DHCPv6 with prefix delegation (over PPP with PPPoE).
    Dhcpv6 {
        /// The prefix length to ask for, such as 56; unset, whatever the ISP delegates.
        reqprefix: Option<u8>,
    },
    /// 6rd: IPv6 in IPv4 to the ISP's border relay, the prefix derived from the IPv4 address.
    #[serde(rename = "6rd")]
    SixRd {
        /// The ISP's 6rd prefix, such as `2602::/24`.
        #[cfg_attr(feature = "ts", ts(as = "String"))]
        prefix: Ipv6Net,
        #[cfg_attr(feature = "ts", ts(as = "String"))]
        border_relay: Ipv4Addr,
        /// The leading bits of the IPv4 address the prefix leaves out (0 to 32).
        ipv4_mask_len: u8,
    },
}

impl WanIpv6 {
    pub const fn is_on(&self) -> bool {
        !matches!(self, Self::Off)
    }
}

/// The hosts a line's health is checked against, by the connectivity test (DNS) and by mwan3
/// (pings): Quad9 and Cloudflare, two independent anycast networks.
pub const HEALTH_TARGETS: [Ipv4Addr; 2] = [Ipv4Addr::new(9, 9, 9, 9), Ipv4Addr::new(1, 1, 1, 1)];

/// Smart queueing (features/wan.md, SQM): cake shapes the WAN a little below the line's rates,
/// so the queue builds on the gateway, where cake keeps it short, instead of in the ISP's gear.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Sqm {
    /// The shaped download rate, in kbit/s.
    pub download_kbps: u32,
    /// The shaped upload rate, in kbit/s.
    pub upload_kbps: u32,
    /// The link layer's overhead per packet, in bytes (Ethernet framing on fiber is 38, with
    /// PPPoE 46; DOCSIS 18); unset, none is added.
    pub overhead: Option<u8>,
    /// Re-measure the line every week at night and suggest new rates when it changed.
    pub autotune: bool,
}

/// The backup line (features/wan.md, Failover): mwan3 checks both lines against
/// [`HEALTH_TARGETS`] and routes over the backup while the WAN fails.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Failover {
    pub link: BackupLink,
    pub proto: BackupProto,
}

/// Where the backup line plugs in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BackupLink {
    /// One of the gateway's Ethernet ports, taken out of the LAN (an LTE router, a second
    /// ISP), on a VLAN when the line is tagged.
    Port { port: String, vlan: Option<u16> },
    /// A phone or modem tethered over USB (RNDIS or CDC Ethernet), whose device is usually
    /// `usb0`.
    Usb { device: String },
}

/// How the backup line gets its address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BackupProto {
    Dhcp,
    Static {
        #[cfg_attr(feature = "ts", ts(as = "String"))]
        address: Ipv4Net,
        #[cfg_attr(feature = "ts", ts(as = "String"))]
        gateway: Ipv4Addr,
        dns: Vec<IpAddr>,
    },
}

/// Services on the gateway.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Services {
    /// Custom local DNS records, besides the automatic `<hostname>.<domain>`.
    pub dns_records: Vec<DnsRecord>,
}

/// The host name that always resolves to the controller on the LAN (`uniwrt.<domain>`): the
/// gateway's dnsmasq holds it, and a custom record can't take it.
pub const CONTROLLER_HOST: &str = "uniwrt";

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
