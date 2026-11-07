//! The `upnpd` package (miniupnpd; features/network.md, Firewall policy; D-079): UPnP IGD and
//! NAT-PMP/PCP for the chosen networks, in secure mode (a client maps ports only to itself),
//! with OpenWrt's stock permissions (ports from 1024 only) and lease file, which the agent's
//! `leases` set reads for the live mappings. The WAN is found as netifd's default route.
//! With UPnP off on a gateway that holds the package, it renders the package switched off.
//! miniupnpd writes its `uuid` into the package on first start; render carries it over
//! ([`preserve`](crate::preserve)).

use uniwrt_uci::{Package, Section};

use crate::{Requirement, plan::Plan};

/// The package UPnP needs, with its init script.
pub(crate) const REQUIRES: Requirement = Requirement {
    package: "miniupnpd-nftables",
    service: Some("miniupnpd"),
};

/// The section miniupnpd's init script reads.
pub(crate) const CONFIG: &str = "config";

/// Where miniupnpd keeps its mappings, as OpenWrt ships it.
pub const LEASE_FILE: &str = "/var/run/miniupnpd.leases";

/// The package, or `None` where there is nothing to own: not the gateway, or UPnP off and no
/// package on the node.
pub(crate) fn render(plan: &Plan<'_>, held: bool) -> Option<Package> {
    plan.wan.as_ref()?;
    let upnp = &plan.site.firewall.upnp;
    if !upnp.enabled && !held {
        return None;
    }
    let mut config = Section::named("upnpd", CONFIG)
        .with("enabled", if upnp.enabled { "1" } else { "0" })
        .with("enable_natpmp", "1")
        .with("enable_upnp", "1")
        .with("secure_mode", "1")
        .with("log_output", "0")
        .with("download", "1024")
        .with("upload", "512");
    if !upnp.networks.is_empty() {
        config = config.with("internal_iface", upnp.networks.join(" "));
    }
    let config = config
        .with("port", "5000")
        .with("upnp_lease_file", LEASE_FILE)
        .with("igdv1", "1");
    let mut package = Package::new("upnpd");
    package.sections = vec![
        config,
        Section::anonymous("perm_rule")
            .with("action", "allow")
            .with("ext_ports", "1024-65535")
            .with("int_addr", "0.0.0.0/0")
            .with("int_ports", "1024-65535")
            .with("comment", "Allow high ports"),
        Section::anonymous("perm_rule")
            .with("action", "deny")
            .with("ext_ports", "0-65535")
            .with("int_addr", "0.0.0.0/0")
            .with("int_ports", "0-65535")
            .with("comment", "Default deny"),
    ];
    Some(package)
}
