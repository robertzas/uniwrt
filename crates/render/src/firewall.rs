//! The `firewall` package (fw4; features/network.md, Firewall policy): a zone per network and
//! one for the WAN, a forwarding for each allowed cell of the policy matrix
//! ([`matrix`](self::matrix)), OpenWrt's stock WAN rules, and on the zones that don't reach
//! the gateway freely the rules that let their clients get addresses and names. fw4 accepts
//! replies to allowed connections in every direction, so "block" and "established" both
//! render as no forwarding. APs run no firewall: theirs has only its defaults, which accept
//! input. Port forwards and the DMZ host are `redirect`s ([`redirect`](crate::redirect)), and
//! the untrusted zones that UPnP serves may reach miniupnpd ([`upnp`](crate::upnp)). The wired
//! backhaul's interfaces have a zone of their own ([`backhaul`](crate::backhaul)), and so has the
//! adoption port ([`adoption`](crate::adoption)).

use uniwrt_model::{
    node::NodeRole,
    site::{
        firewall::ZoneAction,
        network::{Ipv6Mode, Purpose},
    },
};
use uniwrt_uci::{Package, Section};

use crate::{ext::SectionExt, plan::Plan};

const WAN: &str = "wan";

pub(crate) fn render(plan: &Plan<'_>) -> Package {
    let gateway = plan.role == NodeRole::Gateway;
    let mut sections = vec![
        Section::anonymous("defaults")
            .with("syn_flood", "1")
            // An AP's firewall is off; should it ever start, it must not lock the controller
            // out.
            .with("input", if gateway { "REJECT" } else { "ACCEPT" })
            .with("output", "ACCEPT")
            .with("forward", "REJECT"),
    ];
    if gateway {
        for net in &plan.nets {
            let open = trusted(net.network.purpose);
            let policy = if open { "ACCEPT" } else { "REJECT" };
            sections.push(
                Section::anonymous("zone")
                    .with("name", net.id())
                    .with("network", [net.id()])
                    .with("input", policy)
                    .with("output", "ACCEPT")
                    .with("forward", policy),
            );
        }
        sections.push(
            Section::anonymous("zone")
                .with("name", WAN)
                .list("network", crate::failover::zone_networks(plan))
                .with("input", "REJECT")
                .with("output", "ACCEPT")
                .with("forward", "DROP")
                .with("masq", "1")
                .with("mtu_fix", "1"),
        );
        for (src, dest) in matrix(plan) {
            sections.push(
                Section::anonymous("forwarding")
                    .with("src", src)
                    .with("dest", dest),
            );
        }
        let lan = plan
            .nets
            .iter()
            .find(|n| n.network.purpose == Purpose::Default)
            .map_or("lan", |n| n.id());
        sections.extend(stock_rules(lan));
        let upnp = &plan.site.firewall.upnp;
        for net in plan.nets.iter().filter(|n| !trusted(n.network.purpose)) {
            sections.extend(service_rules(net.id(), net.network.ipv6 != Ipv6Mode::Off));
            if upnp.enabled && upnp.networks.iter().any(|id| id == net.id()) {
                sections.extend(upnp_rules(net.id()));
            }
        }
        sections.extend(crate::redirect::sections(plan, WAN));
        sections.extend(crate::backhaul::firewall(plan));
        sections.extend(crate::adoption::firewall(plan));
    }
    let mut package = Package::new("firewall");
    package.sections = sections;
    package
}

/// Networks whose clients reach the gateway and every other network.
/// Whether a network of `purpose` may reach another network (or with `to_wan` the WAN) before
/// the site's policy overrides the cell: trusted networks reach everything, the others only
/// the WAN, and the WAN reaches nothing.
pub const fn seeded(purpose: Purpose, to_wan: bool) -> bool {
    trusted(purpose) || to_wan
}

const fn trusted(purpose: Purpose) -> bool {
    matches!(purpose, Purpose::Default | Purpose::Mgmt)
}

/// The allowed `(from, to)` cells between zones: each network's purpose seeds its row (the
/// default and management networks reach everything, the others the internet only, the WAN
/// nothing), then the site's policies override cells, in order.
pub(crate) fn matrix<'p>(plan: &'p Plan<'_>) -> Vec<(&'p str, &'p str)> {
    let mut zones: Vec<(&str, Option<Purpose>)> = plan
        .nets
        .iter()
        .map(|n| (n.id(), Some(n.network.purpose)))
        .collect();
    zones.push((WAN, None));
    let mut cells = Vec::new();
    for &(from, purpose) in &zones {
        for &(to, _) in &zones {
            if from == to {
                continue;
            }
            let seeded = purpose.is_some_and(|purpose| seeded(purpose, to == WAN));
            let allowed = plan
                .site
                .firewall
                .policies
                .iter()
                .rev()
                .find(|p| p.from == from && p.to == to)
                .map_or(seeded, |p| p.action == ZoneAction::Allow);
            if allowed {
                cells.push((from, to));
            }
        }
    }
    cells
}

fn rule(name: &str) -> Section {
    Section::anonymous("rule").with("name", name)
}

/// OpenWrt's stock rules for the WAN; ESP and ISAKMP pass to the default network.
fn stock_rules(lan: &str) -> Vec<Section> {
    let icmpv6 = |extra: &[&'static str]| {
        let mut types = vec![
            "echo-request",
            "echo-reply",
            "destination-unreachable",
            "packet-too-big",
            "time-exceeded",
            "bad-header",
            "unknown-header-type",
        ];
        types.extend_from_slice(extra);
        types
    };
    vec![
        rule("Allow-DHCP-Renew")
            .with("src", WAN)
            .with("proto", "udp")
            .with("dest_port", "68")
            .with("target", "ACCEPT")
            .with("family", "ipv4"),
        rule("Allow-Ping")
            .with("src", WAN)
            .with("proto", "icmp")
            .with("icmp_type", "echo-request")
            .with("family", "ipv4")
            .with("target", "ACCEPT"),
        rule("Allow-IGMP")
            .with("src", WAN)
            .with("proto", "igmp")
            .with("family", "ipv4")
            .with("target", "ACCEPT"),
        rule("Allow-DHCPv6")
            .with("src", WAN)
            .with("proto", "udp")
            .with("dest_port", "546")
            .with("family", "ipv6")
            .with("target", "ACCEPT"),
        rule("Allow-MLD")
            .with("src", WAN)
            .with("proto", "icmp")
            .with("src_ip", "fe80::/10")
            .with("icmp_type", ["130/0", "131/0", "132/0", "143/0"])
            .with("family", "ipv6")
            .with("target", "ACCEPT"),
        rule("Allow-ICMPv6-Input")
            .with("src", WAN)
            .with("proto", "icmp")
            .list(
                "icmp_type",
                icmpv6(&[
                    "router-solicitation",
                    "neighbour-solicitation",
                    "router-advertisement",
                    "neighbour-advertisement",
                ]),
            )
            .with("limit", "1000/sec")
            .with("family", "ipv6")
            .with("target", "ACCEPT"),
        rule("Allow-ICMPv6-Forward")
            .with("src", WAN)
            .with("dest", "*")
            .with("proto", "icmp")
            .list("icmp_type", icmpv6(&[]))
            .with("limit", "1000/sec")
            .with("family", "ipv6")
            .with("target", "ACCEPT"),
        rule("Allow-IPSec-ESP")
            .with("src", WAN)
            .with("dest", lan)
            .with("proto", "esp")
            .with("target", "ACCEPT"),
        rule("Allow-ISAKMP")
            .with("src", WAN)
            .with("dest", lan)
            .with("dest_port", "500")
            .with("proto", "udp")
            .with("target", "ACCEPT"),
    ]
}

/// What a zone that may not reach the gateway still needs from it: DHCP and DNS, and with
/// IPv6 DHCPv6 and neighbour discovery.
fn service_rules(zone: &str, ipv6: bool) -> Vec<Section> {
    let mut rules = vec![
        rule(&format!("Allow-{zone}-DHCP"))
            .with("src", zone)
            .with("proto", "udp")
            .with("dest_port", "67")
            .with("family", "ipv4")
            .with("target", "ACCEPT"),
        rule(&format!("Allow-{zone}-DNS"))
            .with("src", zone)
            .with("proto", ["tcp", "udp"])
            .with("dest_port", "53")
            .with("target", "ACCEPT"),
    ];
    if ipv6 {
        rules.push(
            rule(&format!("Allow-{zone}-DHCPv6"))
                .with("src", zone)
                .with("proto", "udp")
                .with("dest_port", "547")
                .with("family", "ipv6")
                .with("target", "ACCEPT"),
        );
        rules.push(
            rule(&format!("Allow-{zone}-ND"))
                .with("src", zone)
                .with("proto", "icmp")
                .with(
                    "icmp_type",
                    [
                        "router-solicitation",
                        "neighbour-solicitation",
                        "neighbour-advertisement",
                    ],
                )
                .with("family", "ipv6")
                .with("target", "ACCEPT"),
        );
    }
    rules
}

/// What a zone that may not reach the gateway needs for UPnP: SSDP and NAT-PMP/PCP, and
/// miniupnpd's HTTP port.
fn upnp_rules(zone: &str) -> [Section; 2] {
    [
        rule(&format!("Allow-{zone}-UPnP"))
            .with("src", zone)
            .with("proto", "udp")
            .with("dest_port", "1900 5351")
            .with("target", "ACCEPT"),
        rule(&format!("Allow-{zone}-UPnP-HTTP"))
            .with("src", zone)
            .with("proto", "tcp")
            .with("dest_port", "5000")
            .with("target", "ACCEPT"),
    ]
}
