//! DNS filtering on the gateway (features/network.md, Blocklists; D-133): the main dnsmasq's
//! conf directory, and with clients bypassing the filter the named instances, the unfiltered
//! one and the firewall's redirects to it.

// Tests and their fixtures may panic on what they set up.
#![allow(clippy::unwrap_used)]

mod support;

use insta::assert_snapshot;
use uniwrt_model::{
    node::NodeRole,
    site::{
        SiteConfig,
        node::{ManagedPackage, RawSection, RawValue},
        services::{DohResolver, Upstream},
    },
};
use uniwrt_render::{DNS_BYPASS_PREFIX, Rendered};
use uniwrt_uci::{Package, Section};

use support::*;

fn ok(result: Result<Rendered, Vec<uniwrt_render::Issue>>) -> Rendered {
    result.unwrap_or_else(|issues| panic!("{issues:?}"))
}

fn filtered(bypass: &[&str]) -> SiteConfig {
    let mut site = home_site();
    site.services.blocking.enabled = true;
    site.services.blocking.bypass = bypass.iter().map(|mac| mac.parse().unwrap()).collect();
    site
}

fn gateway(site: &SiteConfig) -> Rendered {
    ok(run(site, GATEWAY, NodeRole::Gateway, &gateway_facts()))
}

fn dnsmasqs(dhcp: &Package) -> Vec<&Section> {
    dhcp.sections_of("dnsmasq").collect()
}

fn bypasses(firewall: &Package) -> Vec<&Section> {
    firewall
        .sections_of("redirect")
        .filter(|s| {
            s.option("name")
                .is_some_and(|n| n.starts_with(DNS_BYPASS_PREFIX))
        })
        .collect()
}

#[test]
fn off_the_gateway_renders_as_before() {
    let rendered = gateway(&home_site());
    let dhcp = &rendered.packages[&ManagedPackage::Dhcp];
    let [main] = dnsmasqs(dhcp)[..] else {
        panic!("one dnsmasq")
    };
    assert_eq!((main.name.as_deref(), main.option("confdir")), (None, None));
    // A bypass list alone does nothing while filtering is off.
    let mut site = filtered(&["02:00:00:00:10:01"]);
    site.services.blocking.enabled = false;
    let rendered = gateway(&site);
    assert_eq!(dnsmasqs(&rendered.packages[&ManagedPackage::Dhcp]).len(), 1);
    assert_eq!(
        bypasses(&rendered.packages[&ManagedPackage::Firewall]).len(),
        0
    );
}

#[test]
fn on_the_main_dnsmasq_reads_the_controller_s_conf_files() {
    let rendered = gateway(&filtered(&[]));
    let dhcp = &rendered.packages[&ManagedPackage::Dhcp];
    let [main] = dnsmasqs(dhcp)[..] else {
        panic!("one dnsmasq")
    };
    assert_eq!(main.name, None);
    assert_eq!(main.option("confdir"), Some("/tmp/dnsmasq.uniwrt.d,*.conf"));
    assert!(
        dhcp.sections_of("dhcp")
            .all(|s| s.option("instance").is_none())
    );
    assert_eq!(
        bypasses(&rendered.packages[&ManagedPackage::Firewall]).len(),
        0
    );
    // APs keep dnsmasq off and untouched.
    let rendered = ok(run(
        &filtered(&["02:00:00:00:10:01"]),
        AP,
        NodeRole::Ap,
        &gateway_facts(),
    ));
    let dhcp = &rendered.packages[&ManagedPackage::Dhcp];
    let [main] = dnsmasqs(dhcp)[..] else {
        panic!("one dnsmasq")
    };
    assert_eq!((main.name.as_deref(), main.option("confdir")), (None, None));
    assert_eq!(
        bypasses(&rendered.packages[&ManagedPackage::Firewall]).len(),
        0
    );
}

#[test]
fn a_bypass_names_the_instances_and_redirects_its_clients_in_every_network() {
    let mut site = filtered(&["02:00:00:00:10:01", "02:00:00:00:10:02"]);
    site.services.upstream = Upstream::Doh {
        resolvers: vec![DohResolver {
            url: "https://dns.quad9.net/dns-query".to_owned(),
            bootstrap: vec![
                "9.9.9.9".parse().unwrap(),
                "149.112.112.112".parse().unwrap(),
            ],
        }],
    };
    let rendered = gateway(&site);
    let dhcp = &rendered.packages[&ManagedPackage::Dhcp];
    let firewall = &rendered.packages[&ManagedPackage::Firewall];
    let mut shown = Package::new("dhcp");
    shown.sections = dnsmasqs(dhcp).into_iter().cloned().collect();
    shown.sections.extend(
        dhcp.sections
            .iter()
            .filter(|s| matches!(s.kind.as_str(), "dhcp" | "boot"))
            .cloned(),
    );
    let mut redirects = Package::new("firewall");
    redirects.sections = bypasses(firewall).into_iter().cloned().collect();
    assert_snapshot!(
        "gateway_bypass",
        format!("{}\n{}", shown.to_file(), redirects.to_file())
    );
    let names: Vec<_> = dnsmasqs(dhcp).iter().map(|s| s.name.as_deref()).collect();
    assert_eq!(names, [Some("main"), Some("unfiltered")]);
    // Every pool, and the WAN's ignored one, belongs to the main instance.
    assert!(
        dhcp.sections_of("dhcp")
            .all(|s| s.option("instance") == Some("main"))
    );
    assert_eq!(bypasses(firewall).len(), site.networks.len());
}

#[test]
fn a_raw_merge_into_dnsmasq_reaches_the_named_main_instance() {
    let mut site = filtered(&["02:00:00:00:10:01"]);
    let gateway_node = site.nodes.iter_mut().find(|n| n.node == GATEWAY).unwrap();
    gateway_node.raw.push(RawSection {
        package: "dhcp".to_owned(),
        kind: "dnsmasq".to_owned(),
        name: None,
        options: [("logdhcp".to_owned(), RawValue::Option("1".to_owned()))]
            .into_iter()
            .collect(),
        merge: true,
    });
    let rendered = gateway(&site);
    let dhcp = &rendered.packages[&ManagedPackage::Dhcp];
    let [main, unfiltered] = dnsmasqs(dhcp)[..] else {
        panic!("two dnsmasqs")
    };
    assert_eq!(main.option("logdhcp"), Some("1"));
    assert_eq!(unfiltered.option("logdhcp"), None);
    // With the ISP's DNS both instances read the WAN's servers.
    assert_eq!(
        unfiltered.option("resolvfile"),
        Some("/tmp/resolv.conf.d/resolv.conf.auto")
    );
    assert_eq!(unfiltered.option("noresolv"), None);
}

#[test]
fn statistics_log_every_query_and_grow_the_gateway_s_log_buffer() {
    let log_size = |rendered: &Rendered| {
        let system = &rendered.packages[&ManagedPackage::System];
        let section = system.sections_of("system").next().unwrap();
        section.option("log_size").unwrap().to_owned()
    };
    let logs = |rendered: &Rendered| -> Vec<Option<String>> {
        dnsmasqs(&rendered.packages[&ManagedPackage::Dhcp])
            .iter()
            .map(|s| s.option("logqueries").map(str::to_owned))
            .collect()
    };
    // Off with filtering, whatever the toggle says.
    let rendered = gateway(&home_site());
    assert_eq!(
        (logs(&rendered), log_size(&rendered)),
        (vec![None], "128".into())
    );
    // On: both instances log, and the buffer holds 256 KiB.
    let mut site = filtered(&["02:00:00:00:10:01"]);
    let rendered = gateway(&site);
    let on = Some("1".to_owned());
    assert_eq!(
        (logs(&rendered), log_size(&rendered)),
        (vec![on.clone(), on], "256".into())
    );
    // An AP keeps its buffer.
    let rendered = ok(run(&site, AP, NodeRole::Ap, &gateway_facts()));
    assert_eq!(log_size(&rendered), "128");
    // Turned off: neither logs.
    site.services.blocking.stats = false;
    let rendered = gateway(&site);
    assert_eq!(
        (logs(&rendered), log_size(&rendered)),
        (vec![None, None], "128".into())
    );
}
