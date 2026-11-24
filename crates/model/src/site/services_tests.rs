//! The gateway's services (schema v12, D-132; v20, D-146): migration, validation and the DDNS
//! secrets.

use std::{collections::BTreeSet, net::IpAddr};

use serde_json::Value;
use uniwrt_core::NodeId;

use super::{
    SecretRef, SiteConfig, check_secret,
    migrate::upgrade,
    node::ManagedPackage,
    services::{
        DOT_PORT, Ddns, DdnsProvider, DdnsSource, DohResolver, DotResolver, EncryptedResolver,
        Upstream,
    },
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
    assert_eq!(config.services.upstream, Upstream::default());
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
fn v20_makes_the_upstream_layers_and_keeps_what_each_kind_asked() {
    let mut v19: Value =
        serde_json::from_str(include_str!("../../fixtures/site/v19.json")).unwrap();
    let upstream = |value: &Value| upgrade(value.clone()).unwrap().services.upstream;
    let doh = upstream(&v19);
    assert!(doh.is_strict());
    assert_eq!(doh.layers(), 1);
    assert_eq!(doh.doh().count(), doh.encrypted.len());
    v19["services"]["upstream"] = serde_json::json!({ "kind": "custom", "servers": ["9.9.9.9"] });
    assert_eq!(
        upstream(&v19),
        Upstream::custom(vec!["9.9.9.9".parse().unwrap()])
    );
    v19["services"]["upstream"] = serde_json::json!({ "kind": "isp" });
    assert_eq!(upstream(&v19), Upstream::default());
    let config = upgrade(v19).unwrap();
    for node in &config.nodes {
        let dhcp = node.managed.contains(&ManagedPackage::Dhcp);
        assert_eq!(node.managed.contains(&ManagedPackage::Stubby), dhcp);
    }
}

fn ips(list: &[&str]) -> Vec<IpAddr> {
    list.iter().map(|s| s.parse().unwrap()).collect()
}

fn doh(url: &str, bootstrap: &[&str]) -> EncryptedResolver {
    EncryptedResolver::Doh(DohResolver {
        url: url.to_owned(),
        bootstrap: ips(bootstrap),
    })
}

fn dot(name: &str, addresses: &[&str]) -> EncryptedResolver {
    EncryptedResolver::Dot(DotResolver {
        name: name.to_owned(),
        addresses: ips(addresses),
        port: DOT_PORT,
        spki: Vec::new(),
    })
}

#[test]
fn the_layers_need_one_in_effect_and_strict_needs_an_encrypted_resolver() {
    let with = |upstream: Upstream| issues_after(|c| c.services.upstream = upstream);
    let none = Upstream {
        isp: false,
        ..Upstream::default()
    };
    assert_eq!(with(none.clone()), [error("services/upstream")]);
    let strict = Upstream {
        strict: true,
        ..Upstream::default()
    };
    assert_eq!(with(strict), [error("services/upstream/strict")]);
    let layered = Upstream {
        encrypted: vec![dot("dns.quad9.net", &["9.9.9.9"])],
        servers: ips(&["192.0.2.53"]),
        isp: true,
        strict: false,
    };
    assert_eq!(layered.layers(), 3);
    assert_eq!(with(layered.clone()), []);
    let strict = Upstream {
        strict: true,
        ..layered
    };
    // The plain layers are kept but left out.
    assert_eq!(
        (strict.layers(), strict.plain(), strict.uses_isp()),
        (1, &[][..], false)
    );
    assert_eq!(with(strict), []);
}

#[test]
fn custom_servers_are_up_to_four_distinct_unicast_addresses() {
    let custom =
        |servers: &[&str]| issues_after(|c| c.services.upstream = Upstream::custom(ips(servers)));
    assert_eq!(custom(&["9.9.9.9", "2620:fe::fe"]), []);
    assert_eq!(custom(&[]), [error("services/upstream")]);
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
    let strict = |resolvers| issues_after(|c| c.services.upstream = Upstream::strict(resolvers));
    assert_eq!(strict(vec![doh("https://1.1.1.1/dns-query", &[])]), []);
    assert_eq!(
        strict(vec![
            doh("https://dns.google/dns-query", &[]),
            doh("http://dns.google/dns-query", &["8.8.8.8"]),
            doh("https://1.1.1.1/dns-query", &["0.0.0.0", "1.0.0.1"]),
            doh("https://1.1.1.1/x", &["1.0.0.1"]),
        ]),
        [
            error("services/upstream/encrypted/0/bootstrap"),
            error("services/upstream/encrypted/1/url"),
            error("services/upstream/encrypted/2/bootstrap/0"),
            error("services/upstream/encrypted/3/bootstrap"),
        ]
    );
    assert_eq!(
        strict(vec![
            doh("https://1.1.1.1/dns-query", &[]),
            doh("https://1.1.1.1/dns-query", &[]),
        ]),
        [error("services/upstream/encrypted/1/url")]
    );
    let five = vec![doh("https://1.1.1.1/dns-query", &[]); 5];
    assert!(strict(five).contains(&error("services/upstream/encrypted")));
}

#[test]
fn a_dot_resolver_needs_a_host_name_addresses_a_port_and_well_formed_pins() {
    let strict = |resolvers| issues_after(|c| c.services.upstream = Upstream::strict(resolvers));
    let pinned = |pins: &[&str], port: u16| {
        let EncryptedResolver::Dot(mut resolver) = dot("one.one.one.one", &["1.1.1.1"]) else {
            unreachable!()
        };
        resolver.spki = pins.iter().map(|p| (*p).to_owned()).collect();
        resolver.port = port;
        EncryptedResolver::Dot(resolver)
    };
    let pin = "yioEpqeR4WtDwE9YxNVnCEkTxIjx6EEIwFSQW+lJsbc=";
    assert_eq!(
        strict(vec![
            dot("dns.quad9.net", &["9.9.9.9", "2620:fe::fe"]),
            pinned(&[pin], 853)
        ]),
        []
    );
    assert_eq!(
        strict(vec![
            dot("quad9", &["9.9.9.9"]),
            dot("dns.google", &[]),
            dot("dns.google", &["8.8.8.8", "8.8.8.8"]),
            pinned(&["sha256/abc", &pin.replace('c', "d")], 0),
        ]),
        [
            error("services/upstream/encrypted/0/name"),
            error("services/upstream/encrypted/1/addresses"),
            error("services/upstream/encrypted/2/name"),
            error("services/upstream/encrypted/2/addresses/1"),
            error("services/upstream/encrypted/3/port"),
            error("services/upstream/encrypted/3/spki/0"),
            error("services/upstream/encrypted/3/spki/1"),
        ]
    );
    let doh = doh("https://1.1.1.1/dns-query", &[]);
    assert_eq!(
        strict(vec![
            dot("dns.quad9.net", &["9.9.9.9"]),
            doh.clone(),
            dot("dns.google", &["8.8.8.8"])
        ]),
        [error("services/upstream/encrypted/2")]
    );
    assert_eq!(
        strict(vec![
            doh,
            dot("dns.quad9.net", &["9.9.9.9"]),
            dot("dns.google", &["8.8.8.8"])
        ]),
        []
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
