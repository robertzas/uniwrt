//! The gateway's services (schema v12, D-132): migration, validation and the DDNS secrets.

use std::collections::BTreeSet;

use serde_json::Value;
use uniwrt_core::NodeId;

use super::{
    SecretRef, SiteConfig, check_secret,
    migrate::upgrade,
    node::ManagedPackage,
    services::{Ddns, DdnsProvider, DdnsSource, DohResolver, Upstream},
    validate::{Severity, validate},
};

fn fixture(version: u32) -> Value {
    let text = match version {
        11 => include_str!("../../fixtures/site/v11.json"),
        _ => include_str!("../../fixtures/site/v12.json"),
    };
    serde_json::from_str(text).unwrap()
}

/// The (path, severity) of the issues under `services` of the v12 fixture after `edit`.
fn issues_after(edit: impl FnOnce(&mut SiteConfig)) -> Vec<(String, Severity)> {
    let mut config = upgrade(fixture(12)).unwrap();
    edit(&mut config);
    validate(&config, &BTreeSet::from([NodeId::new(1), NodeId::new(2)]))
        .into_iter()
        .filter(|issue| issue.path.starts_with("services"))
        .map(|issue| (issue.path, issue.severity))
        .collect()
}

fn error(path: &str) -> (String, Severity) {
    (path.to_owned(), Severity::Error)
}

fn warning(path: &str) -> (String, Severity) {
    (path.to_owned(), Severity::Warning)
}

fn ddns(id: &str, hostname: &str, provider: DdnsProvider) -> Ddns {
    Ddns {
        id: id.to_owned(),
        enabled: true,
        hostname: hostname.to_owned(),
        provider,
        ipv6: false,
        source: DdnsSource::Wan,
        secret: Some(SecretRef::numbered(7)),
    }
}

#[test]
fn v12_keeps_the_isp_s_dns_and_the_ntp_server_and_manages_the_new_packages() {
    let config = upgrade(fixture(11)).unwrap();
    assert_eq!(config.services.upstream, Upstream::Isp);
    assert_eq!(config.services.ddns, []);
    assert!(config.services.ntp_server);
    let gateway = config
        .nodes
        .iter()
        .find(|n| n.node == NodeId::new(1))
        .unwrap();
    assert!(gateway.managed.contains(&ManagedPackage::HttpsDnsProxy));
    assert!(gateway.managed.contains(&ManagedPackage::Ddns));
    for node in &config.nodes {
        let dhcp = node.managed.contains(&ManagedPackage::Dhcp);
        assert_eq!(node.managed.contains(&ManagedPackage::Ddns), dhcp);
    }
}

#[test]
fn the_fixture_s_services_validate() {
    assert_eq!(issues_after(|_| {}), []);
}

#[test]
fn custom_servers_are_one_to_four_distinct_unicast_addresses() {
    let custom = |servers: &[&str]| {
        issues_after(|c| {
            c.services.upstream = Upstream::Custom {
                servers: servers.iter().map(|s| s.parse().unwrap()).collect(),
            };
        })
    };
    assert_eq!(custom(&["9.9.9.9", "2620:fe::fe"]), []);
    assert_eq!(custom(&[]), [error("services/upstream/servers")]);
    assert_eq!(
        custom(&["9.9.9.9", "9.9.9.9", "127.0.0.1"]),
        [
            error("services/upstream/servers/1"),
            error("services/upstream/servers/2")
        ]
    );
    assert_eq!(
        custom(&["1.1.1.1", "1.0.0.1", "8.8.8.8", "8.8.4.4", "9.9.9.9"]),
        [error("services/upstream/servers")]
    );
}

#[test]
fn a_doh_resolver_needs_an_https_url_and_a_bootstrap_for_a_named_host() {
    let doh = |resolvers: Vec<DohResolver>| {
        issues_after(|c| c.services.upstream = Upstream::Doh { resolvers })
    };
    let resolver = |url: &str, bootstrap: &[&str]| DohResolver {
        url: url.to_owned(),
        bootstrap: bootstrap.iter().map(|s| s.parse().unwrap()).collect(),
    };
    assert_eq!(doh(vec![resolver("https://1.1.1.1/dns-query", &[])]), []);
    assert_eq!(doh(vec![]), [error("services/upstream/resolvers")]);
    assert_eq!(
        doh(vec![
            resolver("https://dns.google/dns-query", &[]),
            resolver("http://dns.google/dns-query", &["8.8.8.8"]),
            resolver("https://1.1.1.1/dns-query", &["0.0.0.0", "1.0.0.1"]),
            resolver("https://1.1.1.1/x", &["1.0.0.1"]),
        ]),
        [
            error("services/upstream/resolvers/0/bootstrap"),
            error("services/upstream/resolvers/1/url"),
            error("services/upstream/resolvers/2/bootstrap/0"),
            error("services/upstream/resolvers/3/bootstrap"),
        ]
    );
    assert_eq!(
        doh(vec![
            resolver("https://1.1.1.1/dns-query", &[]),
            resolver("https://1.1.1.1/dns-query", &[]),
        ]),
        [error("services/upstream/resolvers/1/url")]
    );
}

#[test]
fn ddns_entries_need_ids_names_their_provider_s_fields_and_a_secret() {
    let with = |entries: Vec<Ddns>| issues_after(|c| c.services.ddns = entries);
    let cloudflare = |zone: &str| DdnsProvider::Cloudflare {
        zone: zone.to_owned(),
    };
    assert_eq!(
        with(vec![
            ddns("a", "home.example.com", cloudflare("example.com")),
            ddns("b", "example.com", cloudflare("example.com")),
            ddns("c", "x.duckdns.org", DdnsProvider::Duckdns),
            ddns("d", "me.dedyn.io", DdnsProvider::Desec),
            ddns(
                "e",
                "me.ddns.net",
                DdnsProvider::NoIp {
                    username: "me@example.com".to_owned()
                }
            ),
        ]),
        []
    );
    let mut unsecret = ddns("a", "home.example.com", DdnsProvider::Freedns);
    unsecret.secret = None;
    let mut off = unsecret.clone();
    off.id = "off".to_owned();
    off.enabled = false;
    assert_eq!(
        with(vec![
            unsecret,
            off,
            ddns("a", "home.other.org", cloudflare("example.com")),
            ddns("Bad", "x.y.duckdns.org", DdnsProvider::Duckdns),
            ddns("dup", "HOME.example.com", DdnsProvider::Desec),
            ddns(
                "dyn",
                "nodot",
                DdnsProvider::Dynu {
                    username: "has space".to_owned()
                }
            ),
        ]),
        [
            error("services/ddns/0/secret"),
            error("services/ddns/2/id"),
            error("services/ddns/2/provider/zone"),
            error("services/ddns/3/id"),
            error("services/ddns/3/hostname"),
            error("services/ddns/4/hostname"),
            error("services/ddns/5/hostname"),
            error("services/ddns/5/provider/username"),
        ]
    );
}

#[test]
fn a_custom_update_url_needs_its_password_only_where_it_names_it() {
    let custom = |url: &str, secret: bool| {
        issues_after(|c| {
            let mut entry = ddns(
                "own",
                "home.example.com",
                DdnsProvider::Custom {
                    url: url.to_owned(),
                    username: None,
                },
            );
            if !secret {
                entry.secret = None;
            }
            c.services.ddns = vec![entry];
        })
    };
    assert_eq!(
        custom("https://dyn.example/u?h=[DOMAIN]&ip=[IP]", false),
        []
    );
    assert_eq!(
        custom("https://dyn.example/u?h=[DOMAIN]&p=[PASSWORD]", false),
        [error("services/ddns/0/secret")]
    );
    assert_eq!(
        custom("http://dyn.example/u?p=[PASSWORD]", true),
        [warning("services/ddns/0/provider/url")]
    );
    assert_eq!(
        custom("ftp://dyn.example/", true),
        [error("services/ddns/0/provider/url")]
    );
}

#[test]
fn an_ipv6_entry_warns_without_ipv6_on_the_wan() {
    let issues = issues_after(|c| {
        c.wan.ipv6 = super::gateway::WanIpv6::Off;
        let mut entry = ddns("six", "home.example.com", DdnsProvider::Desec);
        entry.ipv6 = true;
        c.services.ddns = vec![entry];
    });
    assert_eq!(issues, [warning("services/ddns/0/ipv6")]);
}

#[test]
fn ddns_secrets_are_slots_by_id() {
    let mut config = upgrade(fixture(12)).unwrap();
    let path = "services/ddns/home/secret";
    let old = config.secret(path).cloned().unwrap();
    assert!(
        config
            .secrets()
            .iter()
            .any(|s| s.path == "services/ddns/duck/secret" && s.value.is_none())
    );
    let new = SecretRef::numbered(99);
    config.replace_secret(&old, &new);
    assert_eq!(config.secret(path), Some(&new));
    *config.secret_mut("services/ddns/duck/secret").unwrap() = Some(old.clone());
    assert_eq!(config.secret("services/ddns/duck/secret"), Some(&old));
    assert!(config.secret_mut("services/ddns/nope/secret").is_none());
    assert!(check_secret(path, "tok-en_123").is_ok());
    assert!(check_secret(path, "").is_err());
    assert!(check_secret(path, "bad\ntoken").is_err());
}
