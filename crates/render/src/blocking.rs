//! DNS filtering on the gateway (features/network.md, Blocklists; D-133). While it is on, the
//! main dnsmasq reads `*.conf` files from [`CONF_DIR`], where the controller pushes the compiled
//! list (a `servers-file`, read again on SIGHUP) and the conf file that loads it; until then
//! the directory is empty and dnsmasq resolves unfiltered. Clients that bypass the filter have
//! their DNS (port 53, TCP and UDP, IPv4 and IPv6) redirected by the firewall, matched by MAC
//! in each network's zone, to a second dnsmasq instance on [`UNFILTERED_PORT`] that forwards to
//! the same upstream and asks the main instance for local names. The instances are then named
//! ([`MAIN_INSTANCE`], [`UNFILTERED_INSTANCE`]) and the main one's DHCP sections name it: with
//! `nonwildcard` dnsmasq shares port 67, so an instance taking the pools would answer DHCP too.
//! While its statistics are on, both instances log every query (`logqueries`, which OpenWrt
//! passes as `--log-queries=extra`) and the gateway's log buffer grows to 256 KiB.

use uniwrt_model::{
    node::NodeRole,
    site::blocking::{CONF_DIR, MAIN_INSTANCE, UNFILTERED_INSTANCE, UNFILTERED_PORT},
};
use uniwrt_uci::{Section, Value};

use crate::{ext::SectionExt, plan::Plan};

/// The firewall redirects' names: `<prefix><network>`.
pub const BYPASS_PREFIX: &str = "uniwrt_dns_bypass_";

/// The main dnsmasq, reading the controller's files while filtering is on, and logging every
/// query while its statistics are on.
pub(crate) fn main(plan: &Plan<'_>, dnsmasq: Section) -> Section {
    let blocking = &plan.site.services.blocking;
    if !blocking.enabled {
        return dnsmasq;
    }
    dnsmasq
        .with("confdir", format!("{CONF_DIR},*.conf"))
        .maybe("logqueries", blocking.logs_queries().then_some("1"))
}

/// The gateway's log buffer in KiB: dnsmasq's query log needs room between the agent's reads.
pub(crate) fn log_size(plan: &Plan<'_>) -> &'static str {
    if plan.role == NodeRole::Gateway && plan.site.services.blocking.logs_queries() {
        "256"
    } else {
        "128"
    }
}

/// The dnsmasq sections of the `dhcp` package (the main instance first), named and pinned to
/// the main instance while a bypass is rendered, with the unfiltered instance after the main
/// one; as they are otherwise.
pub(crate) fn instances(plan: &Plan<'_>, mut sections: Vec<Section>) -> Vec<Section> {
    if !plan.site.services.blocking.bypasses() {
        return sections;
    }
    let Some(main) = sections.iter().position(|s| s.kind == "dnsmasq") else {
        return sections;
    };
    for section in &mut sections {
        if matches!(section.kind.as_str(), "dhcp" | "boot") {
            section.set("instance", MAIN_INSTANCE);
        }
    }
    sections[main].name = Some(MAIN_INSTANCE.to_owned());
    let unfiltered = unfiltered(plan, &sections[main]);
    sections.insert(main + 1, unfiltered);
    sections
}

/// The unfiltered instance: DNS only on [`UNFILTERED_PORT`], the main instance's upstream (its
/// resolv file or servers, in the same order), local names and the controller's from the main
/// instance.
fn unfiltered(plan: &Plan<'_>, main: &Section) -> Section {
    let domain = plan.site.site.domain.as_str();
    let mut section = Section::named("dnsmasq", UNFILTERED_INSTANCE)
        .with("port", UNFILTERED_PORT.to_string())
        .with("domainneeded", "1")
        .with("boguspriv", "1")
        .with("rebind_protection", "1")
        .with("rebind_localhost", "1")
        // Local names come back from the main instance with private addresses.
        .with("rebind_domain", [domain])
        .with("nonwildcard", "1")
        .with("localservice", "1")
        // Never rewrite the gateway's own resolv.conf: the main instance is its resolver.
        .with("localuse", "0")
        .with("cachesize", "1000")
        .with("ednspacket_max", "1232")
        .maybe(
            "logqueries",
            plan.site.services.blocking.logs_queries().then_some("1"),
        );
    if main.option("strictorder") == Some("1") {
        section.set("strictorder", "1");
    }
    match (main.option("noresolv"), main.option("resolvfile")) {
        (Some("1"), _) => section.set("noresolv", "1"),
        (_, Some(file)) => section.set("resolvfile", file),
        _ => {}
    }
    let upstream = main.get("server").map_or(&[][..], Value::items);
    section.list(
        "server",
        std::iter::once(format!("/{domain}/127.0.0.1")).chain(upstream.iter().cloned()),
    )
}

/// The redirects of the bypassed clients' DNS to the unfiltered instance, one per network.
pub(crate) fn firewall(plan: &Plan<'_>) -> Vec<Section> {
    let blocking = &plan.site.services.blocking;
    if !blocking.bypasses() {
        return Vec::new();
    }
    let macs: Vec<String> = blocking.bypass.iter().map(ToString::to_string).collect();
    plan.nets
        .iter()
        .map(|net| {
            Section::anonymous("redirect")
                .with("name", format!("{BYPASS_PREFIX}{}", net.id()))
                .with("target", "DNAT")
                .with("src", net.id())
                .with("family", "any")
                .list("proto", ["tcp", "udp"])
                .list("src_mac", macs.iter().cloned())
                .with("src_dport", "53")
                .with("dest_port", UNFILTERED_PORT.to_string())
        })
        .collect()
}
