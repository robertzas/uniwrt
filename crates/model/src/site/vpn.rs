//! The site's VPN (features/network.md, VPN; D-140): a WireGuard server on the gateway and its
//! peers, remote clients dialing in and other sites' routers. Schema v17 adds it.
//!
//! - **Keys:** the controller makes the server's keypair, each peer's preshared key and a
//!   peer's keypair unless the peer brings its own public key. Private and preshared keys are
//!   vault secrets ([`SecretRef`]); public keys are plain base64 ([`is_key`]).
//! - **Addresses:** IPv4 inside the tunnel. The gateway holds [`WireGuard::address`] (by
//!   default 10.0.250.1/24) and each peer one host of that subnet.
//! - **Routes:** a client's full tunnel sends everything through the gateway, a split one only the
//!   site's networks. A site peer's networks are routed to it.

use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};
use uniwrt_core::Ipv4Net;

use super::SecretRef;

/// The tunnel's UCI interface and its firewall zone.
pub const ZONE: &str = "vpn";

/// The UDP port a new server listens on.
pub const DEFAULT_PORT: u16 = 51820;

/// A new server's address in the tunnel, with the tunnel's subnet.
pub const DEFAULT_ADDRESS: Ipv4Net = Ipv4Net::new_assert(Ipv4Addr::new(10, 0, 250, 1), 24);

/// The keepalive of site peers, in seconds, which keeps their NAT mappings open.
pub const SITE_KEEPALIVE_S: u16 = 25;

/// The most peers a server has.
pub const MAX_PEERS: usize = 250;

/// The site's VPNs.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Vpn {
    /// The WireGuard server; absent until it is set up.
    pub wireguard: Option<WireGuard>,
}

/// The gateway's WireGuard server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct WireGuard {
    pub enabled: bool,
    /// The UDP port it listens on at the WAN.
    pub port: u16,
    /// The gateway's address in the tunnel, with the subnet's prefix, such as 10.0.250.1/24.
    #[cfg_attr(feature = "ts", ts(as = "String"))]
    pub address: Ipv4Net,
    /// What peers dial.
    pub endpoint: Endpoint,
    /// Peers ask the gateway's tunnel address for names.
    pub dns: bool,
    /// The server's private key.
    pub private_key: Option<SecretRef>,
    /// The server's public key, which every peer's configuration names.
    pub public_key: Option<String>,
    pub peers: Vec<Peer>,
}

impl WireGuard {
    /// A new server with the default port and subnet, dialed at `endpoint`, without keys.
    pub fn new(endpoint: Endpoint) -> Self {
        Self {
            enabled: true,
            port: DEFAULT_PORT,
            address: DEFAULT_ADDRESS,
            endpoint,
            dns: true,
            private_key: None,
            public_key: None,
            peers: Vec::new(),
        }
    }

    /// The lowest host of the tunnel's subnet that neither the gateway nor a peer holds.
    pub fn free_address(&self) -> Option<Ipv4Addr> {
        self.address.hosts().find(|&host| {
            host != self.address.addr() && self.peers.iter().all(|peer| peer.address != host)
        })
    }
}

/// What peers dial: the gateway's public name or address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(rename = "VpnEndpoint"))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Endpoint {
    /// One of the site's DDNS names, by its id.
    Ddns { id: String },
    /// A name or address typed in.
    Host { host: String },
}

/// A peer of the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(rename = "VpnPeer"))]
pub struct Peer {
    /// Names its UCI section (`wg_<id>`): `[a-z][a-z0-9_]{0,11}`, never changed once created.
    pub id: String,
    /// For people, such as "Robin's phone".
    pub name: String,
    pub enabled: bool,
    /// Its address in the tunnel.
    #[cfg_attr(feature = "ts", ts(as = "String"))]
    pub address: Ipv4Addr,
    pub public_key: String,
    /// Its private key, where the controller made the keypair, so its configuration can be
    /// shown again.
    pub private_key: Option<SecretRef>,
    pub preshared_key: Option<SecretRef>,
    pub role: PeerRole,
}

/// What a peer is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PeerRole {
    /// A phone or laptop dialing in: a full tunnel sends all its traffic through the gateway,
    /// a split one only what goes to the site's networks.
    Client { full_tunnel: bool },
    /// Another site's router: the gateway routes `networks` to it, and dials it at
    /// `endpoint` where that is known (else it waits to be dialed).
    Site {
        #[cfg_attr(feature = "ts", ts(as = "Vec<String>"))]
        networks: Vec<Ipv4Net>,
        endpoint: Option<SiteEndpoint>,
    },
}

/// Where the gateway dials a site peer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SiteEndpoint {
    /// A DNS name or an address.
    pub host: String,
    pub port: u16,
}

/// Whether `key` is a WireGuard key as `wg` prints it: 32 bytes in padded base64, 44
/// characters.
pub fn is_key(key: &str) -> bool {
    let bytes = key.as_bytes();
    let base64 = |b: &u8| b.is_ascii_alphanumeric() || *b == b'+' || *b == b'/';
    // The 43rd character carries the last byte's 4 low bits and 2 zero bits.
    bytes.len() == 44
        && bytes[43] == b'='
        && bytes[..43].iter().all(base64)
        && b"AEIMQUYcgkosw048".contains(&bytes[42])
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use super::{Endpoint, Peer, PeerRole, WireGuard, is_key};

    #[test]
    fn keys_are_32_bytes_of_padded_base64() {
        assert!(is_key("yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk="));
        assert!(is_key(&format!("{}=", "A".repeat(43))));
        assert!(!is_key("yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBml="));
        assert!(!is_key("yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk"));
        assert!(!is_key(&format!("{}=", "-".repeat(43))));
        assert!(!is_key(""));
    }

    #[test]
    fn the_free_address_skips_the_gateway_and_the_peers() {
        let mut server = WireGuard::new(Endpoint::Host {
            host: "vpn.example.com".to_owned(),
        });
        assert_eq!(server.free_address(), Some(Ipv4Addr::new(10, 0, 250, 2)));
        for n in [2, 4] {
            server.peers.push(Peer {
                id: format!("p{n}"),
                name: format!("Peer {n}"),
                enabled: true,
                address: Ipv4Addr::new(10, 0, 250, n),
                public_key: String::new(),
                private_key: None,
                preshared_key: None,
                role: PeerRole::Client { full_tunnel: true },
            });
        }
        assert_eq!(server.free_address(), Some(Ipv4Addr::new(10, 0, 250, 3)));
        server.address = "10.0.250.2/30".parse().unwrap_or(server.address);
        assert_eq!(server.free_address(), Some(Ipv4Addr::new(10, 0, 250, 1)));
    }
}
