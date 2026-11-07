//! The ports clients opened on the gateway's WAN through UPnP IGD or NAT-PMP/PCP (miniupnpd).

use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PortMapping {
    /// `tcp` or `udp`.
    pub proto: String,
    /// The WAN's port.
    pub external_port: u16,
    /// The client that asked, which the port forwards to.
    #[cfg_attr(feature = "ts", ts(as = "String"))]
    pub ip: Ipv4Addr,
    pub internal_port: u16,
    /// What the client called it, such as `Plex Media Server`.
    pub description: String,
    /// Seconds left when collected; `None` for a mapping without a lease time.
    pub expires_in_s: Option<u64>,
}
