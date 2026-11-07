//! The gateway's firewall (features/network.md, Firewall policy): the policy between networks,
//! port forwards and the DMZ host, and UPnP. Schema v1 has the policy; v5 (M4.2b, D-079) adds
//! the rest.

use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};
use uniwrt_core::Ipv4Net;

/// The firewall. Each network's purpose seeds a matrix of which networks reach which;
/// `policies` override cells of it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FirewallConfig {
    pub policies: Vec<ZonePolicy>,
    /// Ports on the WAN forwarded to hosts on the networks.
    pub forwards: Vec<PortForward>,
    /// The host that gets every connection from the internet no forward claims.
    #[cfg_attr(feature = "ts", ts(as = "Option<String>"))]
    pub dmz: Option<Ipv4Addr>,
    pub upnp: Upnp,
}

/// The name of the DMZ host's fw4 redirect, which no forward may take.
pub const DMZ: &str = "DMZ";

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

/// What a [`ZonePolicy`] does. Replies to connections the other side opened always pass, so
/// `Block` and `Established` render alike.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ZoneAction {
    Allow,
    Block,
    /// Only replies to connections from the other side.
    Established,
}

/// A port forward (DNAT) from the WAN to a host on a network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PortForward {
    /// Its name, unique among the forwards, such as `Game server`.
    pub name: String,
    pub enabled: bool,
    pub proto: ForwardProto,
    /// The WAN's ports.
    pub ports: PortRange,
    /// The host it forwards to, in a network's subnet.
    #[cfg_attr(feature = "ts", ts(as = "String"))]
    pub to: Ipv4Addr,
    /// The host's first port, where it differs from the WAN's; a range keeps its length.
    pub to_port: Option<u16>,
    /// The only addresses it accepts connections from; `None` is anywhere.
    #[cfg_attr(feature = "ts", ts(as = "Option<String>"))]
    pub source: Option<Ipv4Net>,
    /// Hosts on the networks reach it through the WAN's address too (NAT reflection).
    pub reflection: bool,
}

impl PortForward {
    /// The host's ports.
    pub fn to_ports(&self) -> Option<PortRange> {
        let Some(first) = self.to_port else {
            return Some(self.ports);
        };
        let last = first.checked_add(self.ports.last - self.ports.first)?;
        Some(PortRange { first, last })
    }
}

/// The protocols a [`PortForward`] carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ForwardProto {
    Tcp,
    Udp,
    TcpUdp,
}

impl ForwardProto {
    /// Its protocols' names, as fw4 reads them.
    pub const fn names(self) -> &'static [&'static str] {
        match self {
            Self::Tcp => &["tcp"],
            Self::Udp => &["udp"],
            Self::TcpUdp => &["tcp", "udp"],
        }
    }
}

/// Ports `first` to `last`, inclusive; one port when they are equal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PortRange {
    pub first: u16,
    pub last: u16,
}

impl PortRange {
    pub const fn one(port: u16) -> Self {
        Self {
            first: port,
            last: port,
        }
    }

    pub const fn overlaps(self, other: Self) -> bool {
        self.first <= other.last && other.first <= self.last
    }
}

impl std::fmt::Display for PortRange {
    /// `80`, or `8000-8100`, as fw4 reads ports.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.first == self.last {
            write!(f, "{}", self.first)
        } else {
            write!(f, "{}-{}", self.first, self.last)
        }
    }
}

impl std::str::FromStr for PortRange {
    type Err = ();

    /// `80` or `8000-8100`; ports from 1.
    fn from_str(s: &str) -> Result<Self, ()> {
        let port = |p: &str| p.trim().parse::<u16>().ok().filter(|&p| p > 0).ok_or(());
        let range = match s.split_once(['-', ':']) {
            Some((first, last)) => Self {
                first: port(first)?,
                last: port(last)?,
            },
            None => Self::one(port(s)?),
        };
        if range.first <= range.last {
            Ok(range)
        } else {
            Err(())
        }
    }
}

/// UPnP IGD and NAT-PMP/PCP (miniupnpd): clients on the chosen networks open ports on the WAN
/// for themselves, and only for themselves (its secure mode), to ports from 1024.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Upnp {
    pub enabled: bool,
    /// The networks whose clients may open ports, by id.
    pub networks: Vec<String>,
}
