//! The gateway's network services (features/network.md; D-132): the upstream DNS in dnsmasq,
//! https-dns-proxy for DoH, ddns-scripts for DDNS, the NTP server, and what is left when they
//! go off.

// Tests and their fixtures may panic on what they set up.
#![allow(clippy::unwrap_used)]

mod support;

use std::collections::BTreeMap;

use insta::assert_snapshot;
use uniwrt_model::{
    node::NodeRole,
    site::{
        SiteConfig,
        node::ManagedPackage,
        services::{Ddns, DdnsProvider, DdnsSource, DohResolver, Upstream},
    },
};
use uniwrt_render::Rendered;
use uniwrt_uci::Package;

use support::*;

fn ok(result: Result<Rendered, Vec<uniwrt_render::Issue>>) -> Rendered {
    result.unwrap_or_else(|issues| panic!("{issues:?}"))
}

fn doh(url: &str, bootstrap: &[&str]) -> DohResolver {
    DohResolver {
        url: url.to_owned(),
        bootstrap: bootstrap.iter().map(|ip| ip.parse().unwrap()).collect(),
    }
}

/// The home site with DoH through two resolvers and two DDNS names, one of them off.
fn served() -> SiteConfig {
    let mut site = home_site();
    site.services.upstream = Upstream::Doh {
        resolvers: vec![
            doh(
                "https://cloudflare-dns.com/dns-query",
                &["2606:4700:4700::1111", "1.1.1.1"],
            ),
            doh(
                "https://dns.quad9.net/dns-query",
                &["9.9.9.9", "149.112.112.112"],
            ),
        ],
    };
    site.services.ddns = vec![
        Ddns {
            id: "home".to_owned(),
            enabled: true,
            hostname: "home.example.com".to_owned(),
            provider: DdnsProvider::Cloudflare {
                zone: "example.com".to_owned(),
            },
            ipv6: false,
            source: DdnsSource::Wan,
            secret: Some(secret(5)),
        },
        Ddns {
            id: "spare".to_owned(),
            enabled: false,
            hostname: "spare.ddns.net".to_owned(),
            provider: DdnsProvider::NoIp {
                username: "me".to_owned(),
            },
            ipv6: true,
            source: DdnsSource::Web,
            secret: None,
        },
    ];
    site
}

fn requires(rendered: &Rendered) -> Vec<&'static str> {
    rendered.requires.iter().map(|r| r.package).collect()
}

#[test]
fn gateway_doh_and_ddns() {
    let result = run(&served(), GATEWAY, NodeRole::Gateway, &gateway_facts());
    assert_snapshot!("gateway_doh_and_ddns", show(&result));
    let rendered = ok(result);
    assert_eq!(rendered.services.get("https-dns-proxy"), Some(&true));
    assert_eq!(rendered.services.get("ddns"), Some(&true));
    // No-IP's scripts only while its entry runs.
    assert_eq!(
        requires(&rendered),
        [
            "miniupnpd-nftables",
            "https-dns-proxy",
            "ddns-scripts",
            "ddns-scripts-services",
            "ddns-scripts-cloudflare"
        ]
    );
}

#[test]
fn custom_servers_replace_the_isp_s() {
    let mut site = home_site();
    site.services.upstream = Upstream::Custom {
        servers: vec!["9.9.9.9".parse().unwrap(), "2620:fe::fe".parse().unwrap()],
    };
    let rendered = ok(run(&site, GATEWAY, NodeRole::Gateway, &gateway_facts()));
    let dhcp = &rendered.packages[&ManagedPackage::Dhcp];
    let dnsmasq = dhcp.sections_of("dnsmasq").next().unwrap();
    assert_eq!(dnsmasq.option("noresolv"), Some("1"));
    assert_eq!(
        dnsmasq.get("server").unwrap().items(),
        ["9.9.9.9", "2620:fe::fe"]
    );
    assert!(
        !rendered
            .packages
            .contains_key(&ManagedPackage::HttpsDnsProxy)
    );
    assert!(!rendered.packages.contains_key(&ManagedPackage::Ddns));
    // APs forward nothing: their dnsmasq is off.
    let rendered = ok(run(&site, AP, NodeRole::Ap, &gateway_facts()));
    let dnsmasq = rendered.packages[&ManagedPackage::Dhcp]
        .sections_of("dnsmasq")
        .next()
        .unwrap();
    assert_eq!(dnsmasq.option("noresolv"), None);
}

#[test]
fn the_ntp_server_is_handed_out_unless_a_pool_names_its_own_or_it_is_off() {
    let options = |site: &SiteConfig| -> BTreeMap<String, Vec<String>> {
        let rendered = ok(run(site, GATEWAY, NodeRole::Gateway, &gateway_facts()));
        rendered.packages[&ManagedPackage::Dhcp]
            .sections_of("dhcp")
            .filter_map(|s| {
                let options = s.get("dhcp_option")?.items().to_vec();
                Some((s.name.clone()?, options))
            })
            .collect()
    };
    let ntp = |site: &SiteConfig| {
        let rendered = ok(run(site, GATEWAY, NodeRole::Gateway, &gateway_facts()));
        rendered.packages[&ManagedPackage::System]
            .section("ntp")
            .unwrap()
            .option("enable_server")
            .map(str::to_owned)
    };
    let mut site = home_site();
    assert!(
        options(&site)
            .values()
            .all(|o| o.contains(&"42,0.0.0.0".to_owned()))
    );
    assert_eq!(ntp(&site).as_deref(), Some("1"));
    let lan = site.networks[0]
        .ipv4
        .as_mut()
        .unwrap()
        .dhcp
        .as_mut()
        .unwrap();
    lan.options.ntp = vec!["10.0.0.5".parse().unwrap()];
    let id = site.networks[0].id.clone();
    assert_eq!(options(&site)[&id], ["42,10.0.0.5"]);
    site.services.ntp_server = false;
    assert!(options(&site).values().flatten().all(|o| o != "42,0.0.0.0"));
    assert_eq!(ntp(&site).as_deref(), Some("0"));
}

#[test]
fn doh_and_ddns_off_leave_what_the_gateway_holds_inert() {
    let mut current = stock();
    for (name, text) in [
        (
            "https-dns-proxy",
            "config main 'config'\n\toption force_dns '1'\n\nconfig https-dns-proxy\n\
             \toption resolver_url 'https://dns.google/dns-query'\n\toption listen_port '5053'\n",
        ),
        (
            "ddns",
            "config ddns 'global'\n\toption ddns_loglines '250'\n\nconfig service 'myddns_ipv4'\n\
             \toption service_name 'dyndns.org'\n",
        ),
    ] {
        current.insert(name.to_owned(), uniwrt_uci::parse(name, text).unwrap());
    }
    let site = home_site();
    let rendered = ok(run_on(
        &site,
        GATEWAY,
        NodeRole::Gateway,
        &gateway_facts(),
        &current,
    ));
    let kinds = |package: &Package| -> Vec<String> {
        package.sections.iter().map(|s| s.kind.clone()).collect()
    };
    assert_eq!(
        kinds(&rendered.packages[&ManagedPackage::HttpsDnsProxy]),
        ["main"]
    );
    assert_eq!(kinds(&rendered.packages[&ManagedPackage::Ddns]), ["ddns"]);
    assert_eq!(rendered.services.get("https-dns-proxy"), Some(&false));
    assert_eq!(rendered.services.get("ddns"), Some(&false));
    assert!(
        rendered
            .requires
            .iter()
            .all(|r| !r.package.starts_with("ddns") && r.package != "https-dns-proxy")
    );
    // Never on an access point.
    let rendered = ok(run_on(
        &served(),
        AP,
        NodeRole::Ap,
        &gateway_facts(),
        &current,
    ));
    assert!(
        !rendered
            .packages
            .contains_key(&ManagedPackage::HttpsDnsProxy)
    );
    assert!(!rendered.packages.contains_key(&ManagedPackage::Ddns));
}

#[test]
fn a_ddns_secret_missing_from_the_vault_keeps_the_gateway_from_rendering() {
    let mut site = served();
    site.services.ddns[0].secret = Some(secret(9));
    let issues = run(&site, GATEWAY, NodeRole::Gateway, &gateway_facts()).unwrap_err();
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].path, "services/ddns/home");
}
