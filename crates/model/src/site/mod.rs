//! Desired state (features/lifecycle.md, Desired state & revisions): the one typed document the
//! controller owns, [`SiteConfig`], and its history of [`revision`]s.
//!
//! - **Shape:** site settings and the address plan, then [`network::Network`]s, WLANs and the
//!   mesh ([`wireless`]), the WAN and services ([`gateway`], [`services`]), the firewall ([`firewall`]), the
//!   gateway's [`adoption`] port, the [`vpn`], and per node
//!   its overrides ([`node::NodeConfig`]). Schema v1 has every section; the fields are the ones
//!   M3.3 renders, and later WPs extend them through [`migrate`].
//! - **Keys:** networks and WLANs have ids (`[a-z][a-z0-9_]{0,11}`), which name their UCI
//!   sections and devices, so they never change once created; nodes are keyed by `NodeId`.
//! - **Secrets** ([`secrets`](self::secrets)) are never in the document: a field holds a [`SecretRef`], the name of a vault
//!   entry (`config/<32 hex digits>`). A new value gets a new entry, so an older revision still
//!   finds the value it had. [`SiteConfig::secrets`] lists the slots by path, such as
//!   `wlans/home/passphrase`.
//! - **Checks:** [`validate`] finds errors (a save is refused) and warnings; [`diff`] compares two
//!   documents by structure, matching list items by key. [`ipam`] suggests new networks and
//!   reserved addresses under the address plan (D-077); [`export`] is the site as a file and its
//!   import (D-130).

pub mod adoption;
pub mod blocking;
pub mod diff;
pub mod export;
pub mod firewall;
pub mod gateway;
pub mod ipam;
pub mod migrate;
pub mod network;
pub mod node;
pub mod remove;
pub mod replace;
pub mod revision;
pub mod schedules;
mod secrets;
pub mod services;
pub mod steering;
pub mod tz;
pub mod validate;
pub mod vpn;
pub mod wireless;

use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};
use uniwrt_core::{Ipv4Net, Ipv6Net};

use self::{
    adoption::AdoptionPort,
    firewall::{FirewallConfig, Upnp},
    gateway::{Services, WanConfig},
    network::{Dhcp, DhcpOptions, Ipv4Config, Ipv6Clients, Ipv6Mode, Network, Purpose},
    node::NodeConfig,
    schedules::Schedules,
    steering::Steering,
    vpn::Vpn,
    wireless::{MeshConfig, RadioPolicy, Wlan},
};

pub use self::secrets::{SecretSlot, check_secret};

/// The schema version this uniwrt writes; [`migrate::upgrade`] brings older documents to it.
pub const SCHEMA_VERSION: u32 = 18;

/// The site's desired state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SiteConfig {
    /// [`SCHEMA_VERSION`] when written by this uniwrt.
    pub schema: u32,
    pub site: SiteSettings,
    pub addressing: AddressPlan,
    pub networks: Vec<Network>,
    pub wlans: Vec<Wlan>,
    pub mesh: MeshConfig,
    pub radio: RadioPolicy,
    /// Off when left out (schema v8, D-090).
    #[serde(default)]
    pub steering: Steering,
    pub wan: WanConfig,
    pub firewall: FirewallConfig,
    pub services: Services,
    /// The gateway's adoption port; none when left out (schema v10, D-105).
    #[serde(default)]
    pub adoption_port: Option<AdoptionPort>,
    /// Scheduled restarts and LEDs (schema v16, D-138); both off when left out.
    #[serde(default)]
    pub schedules: Schedules,
    /// The WireGuard server (schema v17, D-140); none when left out.
    #[serde(default)]
    pub vpn: Vpn,
    /// Overrides per adopted node; a node without an entry takes the site's settings.
    pub nodes: Vec<NodeConfig>,
}

/// Site-wide settings that every node shares.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SiteSettings {
    pub name: String,
    /// ISO 3166-1 alpha-2 country code, which sets the radios' regulatory domain. Unset until
    /// the user names it: a wrong country is illegal on the air.
    pub country: Option<String>,
    /// IANA time zone, such as `Europe/Berlin` or `UTC`.
    pub timezone: String,
    /// The local DNS domain (`<hostname>.<domain>`).
    pub domain: String,
    /// NTP servers the nodes sync from.
    pub ntp_servers: Vec<String>,
}

/// The site's address space (features/network.md, Address plan).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct AddressPlan {
    /// The private IPv4 space networks are carved from, 10.0.0.0/16 by default.
    #[cfg_attr(feature = "ts", ts(as = "String"))]
    pub ipv4: Ipv4Net,
    /// The site's unique local IPv6 prefix (a random `fdxx::/48`).
    #[cfg_attr(feature = "ts", ts(as = "Option<String>"))]
    pub ula: Option<Ipv6Net>,
}

/// A secret held in the vault, by its name there (`config/<32 hex digits>`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(transparent)]
pub struct SecretRef(String);

impl SecretRef {
    /// The vault names configuration secrets start with.
    pub const PREFIX: &str = "config/";

    /// A reference to `name`, if it is a configuration secret's name.
    pub fn new(name: impl Into<String>) -> Option<Self> {
        let name = name.into();
        Self::is_valid(&name).then_some(Self(name))
    }

    /// The vault entry's name.
    pub fn name(&self) -> &str {
        &self.0
    }

    /// The reference numbered `n`: a placeholder for a secret not sealed yet, such as one that
    /// brownfield import found, until it is sealed under a name of its own.
    pub fn numbered(n: u128) -> Self {
        Self(format!("{}{n:032x}", Self::PREFIX))
    }

    fn is_valid(name: &str) -> bool {
        name.strip_prefix(Self::PREFIX).is_some_and(|id| {
            id.len() == 32
                && id
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
    }
}

impl SiteConfig {
    /// The defaults of a new site: the 10.0.0.0/16 plan with the given ULA, and the default LAN
    /// at 10.0.0.0/24 (gateway .1, DHCP .100–.249); no WLANs, the mesh off, DHCP on the WAN.
    pub fn defaults(ula: Option<Ipv6Net>) -> Self {
        Self {
            schema: SCHEMA_VERSION,
            site: SiteSettings {
                name: "Home".to_owned(),
                country: None,
                timezone: "UTC".to_owned(),
                domain: "lan".to_owned(),
                ntp_servers: (0..4)
                    .map(|n| format!("{n}.openwrt.pool.ntp.org"))
                    .collect(),
            },
            addressing: AddressPlan {
                ipv4: Ipv4Net::new_assert(Ipv4Addr::new(10, 0, 0, 0), 16),
                ula,
            },
            networks: vec![Network {
                id: "lan".to_owned(),
                name: "Default".to_owned(),
                purpose: Purpose::Default,
                vlan: None,
                ipv4: Some(Ipv4Config {
                    address: Ipv4Net::new_assert(Ipv4Addr::new(10, 0, 0, 1), 24),
                    dhcp: Some(Dhcp {
                        start: Ipv4Addr::new(10, 0, 0, 100),
                        end: Ipv4Addr::new(10, 0, 0, 249),
                        lease_time_s: 12 * 3600,
                        reservations: Vec::new(),
                        options: DhcpOptions::default(),
                    }),
                }),
                // As OpenWrt ships: the stable ULA beside the ISP's delegated prefix.
                ipv6: if ula.is_some() {
                    Ipv6Mode::Both
                } else {
                    Ipv6Mode::Delegated
                },
                ipv6_clients: Ipv6Clients::default(),
                isolation: false,
                mdns: true,
                igmp_snooping: false,
            }],
            wlans: Vec::new(),
            mesh: MeshConfig::default(),
            radio: RadioPolicy::default(),
            steering: Steering::default(),
            wan: WanConfig::default(),
            // UPnP on for the main LAN, so consoles and games work from the start (D-079).
            firewall: FirewallConfig {
                upnp: Upnp {
                    enabled: true,
                    networks: vec!["lan".to_owned()],
                },
                ..FirewallConfig::default()
            },
            services: Services::default(),
            adoption_port: None,
            schedules: Schedules::default(),
            vpn: Vpn::default(),
            nodes: Vec::new(),
        }
    }
}

#[cfg(test)]
mod adoption_tests;
#[cfg(test)]
mod blocking_tests;
#[cfg(test)]
mod firewall_tests;
#[cfg(test)]
mod lan_tests;
#[cfg(test)]
mod multicast_tests;
#[cfg(test)]
mod plan_tests;
#[cfg(test)]
mod services_tests;
#[cfg(test)]
mod tailscale_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod vpn_tests;
