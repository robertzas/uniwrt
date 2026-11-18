//! The gateway's network services (features/network.md, DHCP & local DNS and Other services;
//! D-132): where dnsmasq forwards the queries it can't answer, the DDNS names kept pointing at
//! the WAN, and whether the gateway serves time. Schema v12 adds them.

use std::net::IpAddr;

use serde::{Deserialize, Serialize};

use super::SecretRef;

/// Where the gateway's dnsmasq sends the queries it can't answer itself.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Upstream {
    /// The servers the WAN learns (DHCP, PPPoE) or a static WAN names, as OpenWrt ships.
    #[default]
    Isp,
    /// Plain DNS servers, 1 to [`MAX_SERVERS`].
    Custom {
        #[cfg_attr(feature = "ts", ts(as = "Vec<String>"))]
        servers: Vec<IpAddr>,
    },
    /// DNS over HTTPS through https-dns-proxy: a local instance per resolver (1 to
    /// [`MAX_SERVERS`]), which dnsmasq asks in turn.
    Doh { resolvers: Vec<DohResolver> },
}

/// The most upstream servers or DoH resolvers.
pub const MAX_SERVERS: usize = 4;

/// A DNS-over-HTTPS resolver.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct DohResolver {
    /// Its RFC 8484 endpoint, such as `https://cloudflare-dns.com/dns-query`.
    pub url: String,
    /// Plain DNS servers that resolve the URL's host when the proxy starts: 2 to
    /// [`MAX_SERVERS`] (https-dns-proxy stops with one), or none where the host is an address
    /// (the proxy's own list then). The gateway also resolves the NTP servers' names through
    /// them, as DoH needs the right time first.
    #[cfg_attr(feature = "ts", ts(as = "Vec<String>"))]
    pub bootstrap: Vec<IpAddr>,
}

/// A name a DDNS provider keeps pointing at the gateway's address (ddns-scripts).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Ddns {
    /// Names its ddns-scripts section: `[a-z][a-z0-9_]{0,11}`, never changed once created.
    pub id: String,
    pub enabled: bool,
    /// The name kept up to date, such as `home.example.com`.
    pub hostname: String,
    pub provider: DdnsProvider,
    /// An AAAA record from the WAN's IPv6 address rather than an A record.
    pub ipv6: bool,
    /// Where the address comes from.
    pub source: DdnsSource,
    /// The provider's token or password.
    pub secret: Option<SecretRef>,
}

/// A DDNS provider: the curated ones, each with the fields it needs, or any update URL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DdnsProvider {
    /// Cloudflare, with an API token allowed to edit the zone's DNS; the hostname is in `zone`.
    Cloudflare { zone: String },
    /// DuckDNS: the hostname is `<name>.duckdns.org`, the secret the account's token.
    Duckdns,
    /// deSEC (`dedyn.io` or an own domain), with the domain's token.
    Desec,
    /// No-IP, with the account's username and password.
    NoIp { username: String },
    /// Dynu, with the account's username and password (or an IP update password).
    Dynu { username: String },
    /// FreeDNS (afraid.org), with the record's v2 update token.
    Freedns,
    /// Any service with an HTTP(S) update URL; ddns-scripts fills in `[USERNAME]`,
    /// `[PASSWORD]` (the secret), `[DOMAIN]` (the hostname) and `[IP]`.
    Custom {
        url: String,
        username: Option<String>,
    },
}

/// Where a DDNS entry reads the address to publish.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum DdnsSource {
    /// The WAN interface's own address.
    #[default]
    Wan,
    /// A web service that echoes the address it sees: for a gateway behind another router.
    Web,
}
