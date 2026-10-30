//! The names the controller's certificate covers ([`Names`]), and where its local CA may sign
//! names at all ([`Permitted`], the CA's name constraints; D-039).
//!
//! The leaf covers `localhost`, `uniwrt.lan`, the host name (and `<host>.lan`), loopback, the
//! listen addresses, the private addresses the host routes from, and the names configured with
//! `--tls-names`. The CA is constrained to local names and private ranges plus whatever the
//! leaf needed beyond them when the CA was made: a stolen CA key cannot impersonate other sites.

use std::{
    collections::BTreeSet,
    fmt,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket},
    str::FromStr,
};

use ipnet::IpNet;
use serde::{Deserialize, Serialize};

/// The DNS suffixes a local CA may sign under: RFC 6761's and RFC 8375's, plus the ones home
/// routers use (`lan` is OpenWrt's default domain).
const LOCAL_SUFFIXES: [&str; 5] = ["localhost", "lan", "home.arpa", "local", "internal"];
/// Loopback, private (RFC 1918, RFC 4193), shared (RFC 6598, Tailscale's) and link-local.
const LOCAL_RANGES: [&str; 9] = [
    "127.0.0.0/8",
    "10.0.0.0/8",
    "172.16.0.0/12",
    "192.168.0.0/16",
    "100.64.0.0/10",
    "169.254.0.0/16",
    "::1/128",
    "fc00::/7",
    "fe80::/10",
];

/// A certificate's subject names.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Names {
    pub(crate) dns: BTreeSet<String>,
    pub(crate) ips: BTreeSet<IpAddr>,
}

/// One `--tls-names` entry: a DNS name or an IP address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TlsName {
    Dns(String),
    Ip(IpAddr),
}

impl FromStr for TlsName {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if let Ok(ip) = text.parse() {
            return Ok(Self::Ip(ip));
        }
        let name = text.trim_end_matches('.').to_ascii_lowercase();
        if is_dns_name(&name) {
            Ok(Self::Dns(name))
        } else {
            Err(format!("{text:?} is neither an IP address nor a DNS name"))
        }
    }
}

/// What the host says about itself, for [`Names::wanted`].
#[derive(Debug, Default)]
pub(crate) struct Host {
    pub(crate) hostname: Option<String>,
    /// The addresses the host routes from by default.
    pub(crate) addresses: Vec<IpAddr>,
}

impl Host {
    pub(crate) fn detect() -> Self {
        let hostname = std::fs::read_to_string("/proc/sys/kernel/hostname")
            .or_else(|_| std::fs::read_to_string("/etc/hostname"))
            .ok()
            .map(|name| name.trim().to_ascii_lowercase());
        // Connecting a UDP socket only looks up the route; nothing is sent. The targets are
        // documentation addresses, so this names the default route's source address.
        let source = |bind: IpAddr, target: IpAddr| {
            let socket = UdpSocket::bind((bind, 0)).ok()?;
            socket.connect((target, 9)).ok()?;
            socket.local_addr().ok().map(|addr| addr.ip())
        };
        let addresses = [
            source(
                Ipv4Addr::UNSPECIFIED.into(),
                Ipv4Addr::new(192, 0, 2, 1).into(),
            ),
            source(
                Ipv6Addr::UNSPECIFIED.into(),
                Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1).into(),
            ),
        ];
        Self {
            hostname,
            addresses: addresses.into_iter().flatten().collect(),
        }
    }
}

impl Names {
    /// The names the leaf should cover.
    pub(crate) fn wanted(configured: &[TlsName], listen: &[SocketAddr], host: &Host) -> Self {
        let mut names = Self::default();
        names
            .dns
            .extend(["localhost".to_owned(), "uniwrt.lan".to_owned()]);
        names.ips.extend([
            IpAddr::from(Ipv4Addr::LOCALHOST),
            Ipv6Addr::LOCALHOST.into(),
        ]);
        // Docker names a container by its 12-digit ID, which changes whenever it is recreated.
        if let Some(host) = host.hostname.as_deref()
            && is_dns_name(host)
            && !(host.len() == 12 && host.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            names.dns.insert(host.to_owned());
            if !host.contains('.') {
                names.dns.insert(format!("{host}.lan"));
            }
        }
        let local = Permitted::local();
        names
            .ips
            .extend(host.addresses.iter().filter(|ip| local.covers_ip(**ip)));
        names.ips.extend(
            listen
                .iter()
                .map(SocketAddr::ip)
                .filter(|ip| !ip.is_unspecified()),
        );
        for name in configured {
            match name {
                TlsName::Dns(name) => names.dns.insert(name.clone()),
                TlsName::Ip(ip) => names.ips.insert(*ip),
            };
        }
        names
    }
}

impl fmt::Display for Names {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ips = self.ips.iter().map(ToString::to_string);
        let all: Vec<String> = self.dns.iter().cloned().chain(ips).collect();
        f.write_str(&all.join(", "))
    }
}

/// The CA's name constraints: the DNS subtrees and IP ranges it may sign names in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Permitted {
    pub(crate) dns: Vec<String>,
    pub(crate) ips: Vec<IpNet>,
}

impl Permitted {
    /// Local names and private ranges only.
    pub(crate) fn local() -> Self {
        Self {
            dns: LOCAL_SUFFIXES.map(str::to_owned).to_vec(),
            ips: LOCAL_RANGES
                .iter()
                .filter_map(|net| net.parse().ok())
                .collect(),
        }
    }

    /// The local subtrees plus each of `names` they do not cover.
    pub(crate) fn for_ca(names: &Names) -> Self {
        let mut permitted = Self::local();
        let extra_dns: Vec<String> = names
            .dns
            .iter()
            .filter(|name| !permitted.covers_dns(name))
            .cloned()
            .collect();
        let extra_ips: Vec<IpNet> = names
            .ips
            .iter()
            .filter(|ip| !permitted.covers_ip(**ip))
            .map(|ip| IpNet::from(*ip))
            .collect();
        permitted.dns.extend(extra_dns);
        permitted.ips.extend(extra_ips);
        permitted
    }

    /// A subtree covers its own name and every name below it (RFC 5280, 4.2.1.10).
    pub(crate) fn covers_dns(&self, name: &str) -> bool {
        self.dns.iter().any(|base| {
            name == base
                || name
                    .strip_suffix(base.as_str())
                    .is_some_and(|label| label.ends_with('.'))
        })
    }

    pub(crate) fn covers_ip(&self, ip: IpAddr) -> bool {
        self.ips.iter().any(|net| net.contains(&ip))
    }

    /// Splits `names` into those the CA may sign and those it may not.
    pub(crate) fn split(&self, names: &Names) -> (Names, Names) {
        let (mut signable, mut outside) = (Names::default(), Names::default());
        for name in &names.dns {
            let side = if self.covers_dns(name) {
                &mut signable
            } else {
                &mut outside
            };
            side.dns.insert(name.clone());
        }
        for ip in &names.ips {
            let side = if self.covers_ip(*ip) {
                &mut signable
            } else {
                &mut outside
            };
            side.ips.insert(*ip);
        }
        (signable, outside)
    }
}

/// A host name of letters, digits and inner hyphens per label (RFC 1123), at most 253 long.
fn is_dns_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 253
        && name.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use super::{Host, Names, Permitted, TlsName};

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    #[test]
    fn configured_names_parse_as_ips_or_dns_names() {
        assert_eq!("192.168.1.10".parse(), Ok(TlsName::Ip(ip("192.168.1.10"))));
        assert_eq!("fd00::1".parse(), Ok(TlsName::Ip(ip("fd00::1"))));
        assert_eq!(
            "Uniwrt.Example.COM.".parse(),
            Ok(TlsName::Dns("uniwrt.example.com".into()))
        );
        for bad in [
            "",
            "a..b",
            "-a",
            "a-",
            "*.lan",
            "a_b",
            "ü.lan",
            &"a".repeat(64),
        ] {
            assert!(bad.parse::<TlsName>().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_leaf_wants_local_names_the_host_and_configured_names() {
        let host = Host {
            hostname: Some("homeserver".into()),
            addresses: vec![ip("192.168.1.10"), ip("2001:db8::7")],
        };
        let listen = [
            "0.0.0.0:8443".parse().unwrap(),
            "10.1.2.3:8443".parse().unwrap(),
        ];
        let configured = [
            TlsName::Dns("uniwrt.example.com".into()),
            TlsName::Ip(ip("203.0.113.9")),
        ];
        let names = Names::wanted(&configured, &listen, &host);
        assert_eq!(
            names.to_string(),
            "homeserver, homeserver.lan, localhost, uniwrt.example.com, uniwrt.lan, \
             10.1.2.3, 127.0.0.1, 192.168.1.10, 203.0.113.9, ::1"
        );
    }

    #[test]
    fn docker_container_ids_are_not_host_names() {
        let host = Host {
            hostname: Some("0123456789ab".into()),
            addresses: vec![],
        };
        let names = Names::wanted(&[], &[], &host);
        assert_eq!(names.to_string(), "localhost, uniwrt.lan, 127.0.0.1, ::1");
    }

    #[test]
    fn the_ca_is_constrained_to_local_names_plus_the_extras() {
        let host = Host {
            hostname: Some("homeserver".into()),
            addresses: vec![ip("192.168.1.10")],
        };
        let configured = [
            TlsName::Dns("uniwrt.example.com".into()),
            TlsName::Dns("nas.home.arpa".into()),
            TlsName::Ip(ip("203.0.113.9")),
        ];
        let names = Names::wanted(&configured, &[], &host);
        let permitted = Permitted::for_ca(&names);
        let local = Permitted::local();
        assert_eq!(&permitted.dns[..local.dns.len()], &local.dns[..]);
        assert_eq!(
            &permitted.dns[local.dns.len()..],
            ["homeserver", "uniwrt.example.com"]
        );
        assert_eq!(
            &permitted.ips[local.ips.len()..],
            ["203.0.113.9/32".parse().unwrap()]
        );

        let (signable, outside) = permitted.split(&names);
        assert_eq!(signable, names);
        assert_eq!(outside, Names::default());

        for name in [
            "lan",
            "a.lan",
            "a.b.home.arpa",
            "homeserver",
            "x.uniwrt.example.com",
        ] {
            assert!(permitted.covers_dns(name), "{name}");
        }
        for name in [
            "plan",
            "example.com",
            "evil-homeserver",
            "homeserver.evil.com",
        ] {
            assert!(!permitted.covers_dns(name), "{name}");
        }
        assert!(permitted.covers_ip(ip("172.31.255.255")));
        assert!(permitted.covers_ip(ip("fd12::1")));
        assert!(!permitted.covers_ip(ip("172.32.0.1")));
        assert!(!permitted.covers_ip(ip("203.0.113.10")));
        assert!(!permitted.covers_ip(ip("2001:db8::1")));
    }

    #[test]
    fn names_outside_the_constraints_are_split_off() {
        let permitted = Permitted::local();
        let mut names = Names::default();
        names.dns.extend(["uniwrt.lan".into(), "newhost".into()]);
        names.ips.extend([ip("10.0.0.2"), ip("198.51.100.1")]);
        let (signable, outside) = permitted.split(&names);
        assert_eq!(signable.to_string(), "uniwrt.lan, 10.0.0.2");
        assert_eq!(outside.to_string(), "newhost, 198.51.100.1");
    }
}
