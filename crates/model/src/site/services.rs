//! The gateway's network services (features/network.md, DHCP & local DNS and Other services;
//! D-132): where dnsmasq forwards the queries it can't answer, the DDNS names kept pointing at
//! the WAN, and whether the gateway serves time. Schema v12 adds them; v20 makes the upstream
//! layers (D-146).

use std::net::IpAddr;

use serde::{Deserialize, Serialize};

use super::SecretRef;

/// Where the gateway's dnsmasq sends the queries it can't answer itself (D-132, D-146): layers
/// tried in a fixed order, the encrypted resolvers, then the site's plain servers, then the
/// WAN's, each in the order given. [`strict`](Self::strict) keeps queries on the encrypted
/// layer alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Upstream {
    /// DNS over HTTPS and over TLS, 0 to [`MAX_SERVERS`], first asked first.
    pub encrypted: Vec<EncryptedResolver>,
    /// Plain DNS servers, 0 to [`MAX_SERVERS`], after the encrypted resolvers.
    #[cfg_attr(feature = "ts", ts(as = "Vec<String>"))]
    pub servers: Vec<IpAddr>,
    /// The servers the WAN learns (DHCP, PPPoE) or a static WAN names, last, as OpenWrt ships.
    pub isp: bool,
    /// Only the encrypted resolvers are asked: the plain layers are kept but left out, so no
    /// query leaves the gateway unencrypted. Needs an encrypted resolver.
    pub strict: bool,
}

impl Default for Upstream {
    /// The WAN's servers alone, as OpenWrt ships.
    fn default() -> Self {
        Self {
            encrypted: Vec::new(),
            servers: Vec::new(),
            isp: true,
            strict: false,
        }
    }
}

impl Upstream {
    /// Plain servers only.
    pub fn custom(servers: Vec<IpAddr>) -> Self {
        Self {
            servers,
            isp: false,
            ..Self::default()
        }
    }

    /// These encrypted resolvers alone.
    pub fn strict(encrypted: Vec<EncryptedResolver>) -> Self {
        Self {
            encrypted,
            servers: Vec::new(),
            isp: false,
            strict: true,
        }
    }

    /// Whether only the encrypted resolvers are asked.
    pub fn is_strict(&self) -> bool {
        self.strict && !self.encrypted.is_empty()
    }

    /// The plain servers asked, after the encrypted resolvers: none while strict.
    pub fn plain(&self) -> &[IpAddr] {
        if self.is_strict() { &[] } else { &self.servers }
    }

    /// Whether the WAN's servers are asked, last.
    pub fn uses_isp(&self) -> bool {
        self.isp && !self.is_strict()
    }

    /// How many layers are asked: encrypted, plain, the WAN's.
    pub fn layers(&self) -> usize {
        usize::from(!self.encrypted.is_empty())
            + usize::from(!self.plain().is_empty())
            + usize::from(self.uses_isp())
    }

    /// The DoH resolvers, in order.
    pub fn doh(&self) -> impl Iterator<Item = &DohResolver> + Clone {
        self.encrypted.iter().filter_map(|e| match e {
            EncryptedResolver::Doh(r) => Some(r),
            EncryptedResolver::Dot(_) => None,
        })
    }

    /// The DoT resolvers, in order.
    pub fn dot(&self) -> impl Iterator<Item = &DotResolver> + Clone {
        self.encrypted.iter().filter_map(|e| match e {
            EncryptedResolver::Dot(r) => Some(r),
            EncryptedResolver::Doh(_) => None,
        })
    }
}

/// The most upstream servers, encrypted resolvers, bootstrap servers or DoT addresses.
pub const MAX_SERVERS: usize = 4;

/// The port DNS over TLS listens on unless a resolver says otherwise (RFC 7858).
pub const DOT_PORT: u16 = 853;

/// An encrypted resolver.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "protocol", rename_all = "snake_case")]
pub enum EncryptedResolver {
    /// DNS over HTTPS through https-dns-proxy: a local instance of its own.
    Doh(DohResolver),
    /// DNS over TLS through stubby, which holds every DoT resolver.
    Dot(DotResolver),
}

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

/// A DNS-over-TLS resolver (RFC 7858).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct DotResolver {
    /// The name its certificate must carry, such as `dns.quad9.net`.
    pub name: String,
    /// Its addresses, 1 to [`MAX_SERVERS`], asked in order.
    #[cfg_attr(feature = "ts", ts(as = "Vec<String>"))]
    pub addresses: Vec<IpAddr>,
    /// Its TCP port, [`DOT_PORT`] as a rule.
    pub port: u16,
    /// Pins of its public key (the base64 SHA-256 of the SPKI), 0 to [`MAX_SERVERS`]: with
    /// any, the key must match one of them besides the certificate checking out.
    pub spki: Vec<String>,
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
