//! The adoption port (features/adoption.md, Path C; D-105): a gateway port the admin picks
//! becomes an isolated segment where a factory-default OpenWrt device, which answers at
//! [`AdoptionPort::FACTORY`], can be adopted on the bench. The gateway holds an address in
//! 192.168.1.0/24, the subnet the address plan keeps free for factory-default devices, so the
//! segment never meets the site's networks.

use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};
use uniwrt_core::Ipv4Net;

/// The gateway's port that serves as the adoption port, and its address there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct AdoptionPort {
    /// One of the gateway's Ethernet ports that is a device of its own, such as `lan4`.
    pub port: String,
    /// The gateway's address on the segment, in 192.168.1.0/24 ([`AdoptionPort::DEFAULT`]).
    #[cfg_attr(feature = "ts", ts(as = "String"))]
    pub address: Ipv4Addr,
}

impl AdoptionPort {
    /// Where factory-default OpenWrt answers.
    pub const FACTORY: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 1);
    /// The gateway's address on the segment unless the admin picks another.
    pub const DEFAULT: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 250);
    /// The segment's subnet: factory-default OpenWrt's LAN.
    pub const SUBNET: Ipv4Net = Ipv4Net::new_assert(Ipv4Addr::new(192, 168, 1, 0), 24);

    /// `port` at the default address.
    pub fn new(port: impl Into<String>) -> Self {
        Self {
            port: port.into(),
            address: Self::DEFAULT,
        }
    }

    /// The gateway's address with the segment's prefix, as `192.168.1.250/24`.
    pub fn interface(&self) -> Ipv4Net {
        Ipv4Net::new_assert(self.address, Self::SUBNET.prefix_len())
    }
}
