//! The gateway's network services (D-132): the upstream DNS's layers (D-146) and DDNS.

use std::{collections::BTreeSet, net::IpAddr};

use super::{
    Issues,
    names::{is_dns_name, is_id},
};
use crate::site::{
    SiteConfig,
    gateway::WanIpv6,
    services::{Ddns, DdnsProvider, DohResolver, EncryptedResolver, MAX_SERVERS},
};

/// The most DDNS entries.
const MAX_DDNS: usize = 16;

pub(super) fn upstream(config: &SiteConfig, issues: &mut Issues) {
    let at = "services/upstream";
    let upstream = &config.services.upstream;
    if upstream.encrypted.len() > MAX_SERVERS {
        issues.error(
            format!("{at}/encrypted"),
            format!("At most {MAX_SERVERS} encrypted resolvers"),
        );
    }
    if upstream.servers.len() > MAX_SERVERS {
        issues.error(
            format!("{at}/servers"),
            format!("At most {MAX_SERVERS} DNS servers"),
        );
    }
    servers_ok(&upstream.servers, &format!("{at}/servers"), issues);
    if upstream.strict && upstream.encrypted.is_empty() {
        issues.error(
            format!("{at}/strict"),
            "Encrypted-only needs an encrypted resolver",
        );
    } else if upstream.layers() == 0 {
        issues.error(
            at,
            "Choose where queries go: encrypted resolvers, DNS servers or the ISP's",
        );
    }
    let mut urls = BTreeSet::new();
    let mut dots = BTreeSet::new();
    // stubby asks every DoT resolver in turn, in the first one's place among the encrypted.
    let mut dot_run = None;
    for (i, resolver) in upstream.encrypted.iter().enumerate() {
        let at = format!("{at}/encrypted/{i}");
        match resolver {
            EncryptedResolver::Doh(doh) => {
                doh_ok(doh, &at, &mut urls, issues);
                if dot_run == Some(true) {
                    dot_run = Some(false);
                }
            }
            EncryptedResolver::Dot(dot) => {
                if dot_run == Some(false) {
                    issues.error(
                        at.clone(),
                        "Keep the DoT resolvers next to each other: they are asked in turn, in the first one's place",
                    );
                }
                dot_run.get_or_insert(true);
                if !(is_dns_name(&dot.name) && dot.name.contains('.')) {
                    issues.error(
                        format!("{at}/name"),
                        "A DoT resolver names the host its certificate is for, such as dns.quad9.net",
                    );
                } else if !dots.insert((dot.name.as_str(), dot.port)) {
                    issues.error(format!("{at}/name"), "This resolver is named twice");
                }
                if dot.addresses.is_empty() || dot.addresses.len() > MAX_SERVERS {
                    issues.error(
                        format!("{at}/addresses"),
                        format!("Name 1 to {MAX_SERVERS} addresses"),
                    );
                }
                servers_ok(&dot.addresses, &format!("{at}/addresses"), issues);
                if dot.port == 0 {
                    issues.error(format!("{at}/port"), "Name a port from 1 to 65535");
                }
                if dot.spki.len() > MAX_SERVERS {
                    issues.error(
                        format!("{at}/spki"),
                        format!("At most {MAX_SERVERS} key pins"),
                    );
                }
                for (j, pin) in dot.spki.iter().enumerate() {
                    if !is_spki_pin(pin) {
                        issues.error(
                            format!("{at}/spki/{j}"),
                            "A key pin is a SHA-256 in base64: 43 characters and =",
                        );
                    }
                }
            }
        }
    }
}

/// A DoH resolver: an https:// URL named once, with the bootstrap servers its host needs.
fn doh_ok<'a>(
    resolver: &'a DohResolver,
    at: &str,
    urls: &mut BTreeSet<&'a str>,
    issues: &mut Issues,
) {
    match doh_host(&resolver.url) {
        None => issues.error(
            format!("{at}/url"),
            "A DoH resolver is an https:// URL, such as https://dns.quad9.net/dns-query",
        ),
        Some(host) => {
            if !urls.insert(resolver.url.as_str()) {
                issues.error(format!("{at}/url"), "This resolver is named twice");
            }
            // https-dns-proxy stops with one bootstrap server (c-ares wants a socket more than
            // it has servers; seen on 25.12's 2026.05.06).
            let named = host.parse::<IpAddr>().is_err();
            if resolver.bootstrap.len() == 1 || (named && resolver.bootstrap.is_empty()) {
                issues.error(
                    format!("{at}/bootstrap"),
                    "Name at least two plain DNS servers that resolve the resolver's host",
                );
            }
        }
    }
    if resolver.bootstrap.len() > MAX_SERVERS {
        issues.error(
            format!("{at}/bootstrap"),
            format!("At most {MAX_SERVERS} bootstrap servers"),
        );
    }
    servers_ok(&resolver.bootstrap, &format!("{at}/bootstrap"), issues);
}

/// A key pin as stubby takes it after `sha256/`: 32 bytes in padded base64.
pub fn is_spki_pin(pin: &str) -> bool {
    let bytes = pin.as_bytes();
    bytes.len() == 44
        && bytes[43] == b'='
        && bytes[..43]
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'+' || *b == b'/')
        // The last digit holds 2 bits of padding: only A, E, I, … (the multiples of 4).
        && b"AEIMQUYcgkosw048".contains(&bytes[42])
}

/// Each server is a unicast address, named once.
fn servers_ok(servers: &[IpAddr], at: &str, issues: &mut Issues) {
    let mut seen = BTreeSet::new();
    for (i, ip) in servers.iter().enumerate() {
        if ip.is_unspecified() || ip.is_multicast() || ip.is_loopback() {
            issues.error(format!("{at}/{i}"), format!("{ip} is not a DNS server"));
        } else if !seen.insert(ip) {
            issues.error(format!("{at}/{i}"), format!("{ip} is named twice"));
        }
    }
}

/// The host of a DoH resolver's URL: `https://<host>[:port][/path]`, without characters a UCI
/// value or a URL wouldn't carry plainly; a bracketed IPv6 address comes without its brackets.
pub fn doh_host(url: &str) -> Option<&str> {
    if url.len() > 256
        || url
            .bytes()
            .any(|b| !(b'!'..=b'~').contains(&b) || b"'\"\\,".contains(&b))
    {
        return None;
    }
    let rest = url.strip_prefix("https://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let (host, port) = match authority.strip_prefix('[') {
        Some(v6) => {
            let (host, after) = v6.split_once(']')?;
            host.parse::<std::net::Ipv6Addr>().ok()?;
            (host, after.strip_prefix(':'))
        }
        None => match authority.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        },
    };
    if port.is_some_and(|port| {
        port.parse::<u16>().is_ok_and(|p| p == 0) || port.parse::<u16>().is_err()
    }) {
        return None;
    }
    (host.parse::<IpAddr>().is_ok() || (is_dns_name(host) && host.contains('.'))).then_some(host)
}

pub(super) fn ddns(config: &SiteConfig, issues: &mut Issues) {
    let entries = &config.services.ddns;
    if entries.len() > MAX_DDNS {
        issues.error("services/ddns", format!("At most {MAX_DDNS} DDNS names"));
    }
    let mut ids = BTreeSet::new();
    let mut names = BTreeSet::new();
    for (i, ddns) in entries.iter().enumerate() {
        let at = format!("services/ddns/{i}");
        if !is_id(&ddns.id) {
            issues.error(
                format!("{at}/id"),
                "An id is a lowercase letter, then up to 11 lowercase letters, digits or _",
            );
        } else if !ids.insert(ddns.id.as_str()) {
            issues.error(format!("{at}/id"), format!("{:?} is taken", ddns.id));
        }
        let hostname = ddns.hostname.to_ascii_lowercase();
        if !is_dns_name(&hostname) || !hostname.contains('.') {
            issues.error(
                format!("{at}/hostname"),
                "A full DNS name, such as home.example.com",
            );
        } else if ddns.enabled && !names.insert((hostname.clone(), ddns.ipv6)) {
            issues.error(
                format!("{at}/hostname"),
                format!(
                    "Another entry already keeps the {} record of {hostname}",
                    if ddns.ipv6 { "AAAA" } else { "A" }
                ),
            );
        }
        provider(ddns, &hostname, &at, issues);
        if ddns.enabled && needs_secret(&ddns.provider) && ddns.secret.is_none() {
            issues.error(format!("{at}/secret"), secret_label(&ddns.provider));
        }
        if ddns.enabled && ddns.ipv6 && config.wan.ipv6 == WanIpv6::Off {
            issues.warn(
                format!("{at}/ipv6"),
                "The WAN has IPv6 off, so there is no IPv6 address to publish",
            );
        }
    }
}

fn provider(ddns: &Ddns, hostname: &str, at: &str, issues: &mut Issues) {
    let at = format!("{at}/provider");
    match &ddns.provider {
        DdnsProvider::Cloudflare { zone } => {
            let zone = zone.to_ascii_lowercase();
            if !is_dns_name(&zone) || !zone.contains('.') {
                issues.error(
                    format!("{at}/zone"),
                    "The zone is a domain, such as example.com",
                );
            } else if hostname != zone && !hostname.ends_with(&format!(".{zone}")) {
                issues.error(
                    format!("{at}/zone"),
                    format!("{hostname} is not in the zone {zone}"),
                );
            }
        }
        DdnsProvider::Duckdns => {
            let label = hostname.strip_suffix(".duckdns.org");
            if label.is_none_or(|label| label.is_empty() || label.contains('.')) {
                issues.error(
                    at.replace("/provider", "/hostname"),
                    "A DuckDNS name is <name>.duckdns.org",
                );
            }
        }
        DdnsProvider::NoIp { username } | DdnsProvider::Dynu { username } => {
            username_ok(Some(username), true, &at, issues);
        }
        DdnsProvider::Custom { url, username } => {
            if !update_url_ok(url) {
                issues.error(
                    format!("{at}/url"),
                    "An http:// or https:// URL; ddns-scripts fills in [DOMAIN], [IP], [USERNAME] and [PASSWORD]",
                );
            } else if url.starts_with("http://") && url.contains("[PASSWORD]") {
                issues.warn(
                    format!("{at}/url"),
                    "This URL sends the password unencrypted: use https:// if the service has it",
                );
            }
            username_ok(username.as_ref(), false, &at, issues);
        }
        DdnsProvider::Desec | DdnsProvider::Freedns => {}
    }
}

fn username_ok(username: Option<&String>, required: bool, at: &str, issues: &mut Issues) {
    match username {
        Some(name)
            if name.is_empty()
                || name.len() > 128
                || name.bytes().any(|b| !(b'!'..=b'~').contains(&b)) =>
        {
            issues.error(
                format!("{at}/username"),
                "A username is 1 to 128 printable characters without spaces",
            );
        }
        None if required => issues.error(format!("{at}/username"), "Name the account"),
        _ => {}
    }
}

/// An update URL ddns-scripts can call: http(s), up to 512 printable characters without
/// quotes or spaces.
fn update_url_ok(url: &str) -> bool {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"));
    rest.is_some_and(|rest| !rest.is_empty() && !rest.starts_with('/'))
        && url.len() <= 512
        && url
            .bytes()
            .all(|b| (b'!'..=b'~').contains(&b) && !b"'\"\\".contains(&b))
}

/// Whether the provider's update needs the secret.
pub fn needs_secret(provider: &DdnsProvider) -> bool {
    match provider {
        DdnsProvider::Custom { url, .. } => url.contains("[PASSWORD]"),
        _ => true,
    }
}

fn secret_label(provider: &DdnsProvider) -> &'static str {
    match provider {
        DdnsProvider::Cloudflare { .. } => {
            "Cloudflare needs an API token that may edit the zone's DNS"
        }
        DdnsProvider::Duckdns => "DuckDNS needs the account's token",
        DdnsProvider::Desec => "deSEC needs the domain's token",
        DdnsProvider::NoIp { .. } | DdnsProvider::Dynu { .. } => "Enter the account's password",
        DdnsProvider::Freedns => "FreeDNS needs the record's update token",
        DdnsProvider::Custom { .. } => "The URL names [PASSWORD]: enter it",
    }
}

#[cfg(test)]
mod tests {
    use super::{doh_host, is_spki_pin};

    #[test]
    fn a_key_pin_is_a_sha_256_in_padded_base64() {
        assert!(is_spki_pin("yioEpqeR4WtDwE9YxNVnCEkTxIjx6EEIwFSQW+lJsbc="));
        for bad in [
            "",
            "sha256/yioEpqeR4WtDwE9YxNVnCEkTxIjx6EEIwFSQW+lJsbc=",
            "yioEpqeR4WtDwE9YxNVnCEkTxIjx6EEIwFSQW+lJsbd=",
            "yioEpqeR4WtDwE9YxNVnCEkTxIjx6EEIwFSQW+lJsbc",
            "yioEpqeR4WtDwE9YxNVnCEkTxIjx6EEIwFSQW-lJsbc=",
        ] {
            assert!(!is_spki_pin(bad), "{bad}");
        }
    }

    #[test]
    fn doh_urls_name_a_host() {
        assert_eq!(
            doh_host("https://dns.quad9.net/dns-query"),
            Some("dns.quad9.net")
        );
        assert_eq!(doh_host("https://1.1.1.1/dns-query"), Some("1.1.1.1"));
        assert_eq!(
            doh_host("https://[2606:4700::1111]:443/dns-query"),
            Some("2606:4700::1111")
        );
        assert_eq!(
            doh_host("https://dns.nextdns.io/abc123"),
            Some("dns.nextdns.io")
        );
        assert_eq!(doh_host("https://doh.example:8443"), Some("doh.example"));
        for bad in [
            "http://dns.google/dns-query",
            "https://",
            "https://localhost/dns-query",
            "https://dns.google:0/x",
            "https://dns.google/a b",
            "https://dns.google/a'b",
            "https://[nope]/x",
        ] {
            assert_eq!(doh_host(bad), None, "{bad}");
        }
    }
}
