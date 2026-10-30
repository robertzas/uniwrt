//! DHCP leases the node handed out: dnsmasq's for IPv4, odhcpd's for DHCPv6 (and IPv4 where it
//! serves DHCPv4).

use std::net::IpAddr;

use serde::{Deserialize, Serialize};
use uniwrt_core::MacAddr;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LeaseFacts {
    pub ip: IpAddr,
    /// Unknown for a DHCPv6 lease whose DUID carries no MAC address.
    pub mac: Option<MacAddr>,
    pub hostname: Option<String>,
    /// The DHCPv4 client identifier or the DHCPv6 DUID, in hex.
    pub client_id: Option<String>,
    /// The device it was leased on, such as `br-lan`, when the server says.
    pub device: Option<String>,
    /// Seconds left when collected; `None` for a lease that never expires.
    pub expires_in_s: Option<u64>,
}
