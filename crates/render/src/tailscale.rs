//! Tailscale on the gateway (features/network.md, VPN; D-140), while it is on: in `firewall`
//! the `tailscale` zone over the device `tailscaled` makes (`tailscale0`; no netifd interface:
//! the daemon brings the device up and addresses it), trusted like WireGuard's `vpn`, its row in
//! the matrix seeded like the default LAN's ([`matrix`](crate::firewall::matrix)), and the WAN's
//! rule for the package's UDP port, which lets peers connect directly rather than through a
//! relay. The `tailscale` package brings the daemon and `kmod-tun`. The daemon's own settings
//! (its prefs: routes, exit node, name) are not UCI: the controller sets them through
//! `tailscale set` once it runs. Off renders nothing (the zone goes; the package stays).

use uniwrt_model::{
    node::NodeRole,
    site::{
        SiteConfig,
        vpn::tailscale::{DEVICE, PORT, Tailscale, ZONE},
    },
};
use uniwrt_uci::Section;

use crate::{Requirement, plan::Plan};

/// The daemon and its CLI; its init script is on once installed.
pub(crate) const REQUIRES: Requirement = Requirement {
    package: "tailscale",
    service: None,
};

/// The gateway's Tailscale, where it is on.
pub(crate) fn plan(site: &SiteConfig, role: NodeRole) -> Option<&Tailscale> {
    site.vpn
        .tailscale
        .as_ref()
        .filter(|t| t.enabled && role == NodeRole::Gateway)
}

/// The zone of `tailscale0`: trusted, as the default LAN's.
pub(crate) fn zone(plan: &Plan<'_>) -> Option<Section> {
    plan.tailscale?;
    Some(
        Section::anonymous("zone")
            .with("name", ZONE)
            .with("device", [DEVICE])
            .with("input", "ACCEPT")
            .with("output", "ACCEPT")
            .with("forward", "ACCEPT"),
    )
}

/// The WAN's rule for the daemon's port.
pub(crate) fn rules(plan: &Plan<'_>, wan: &str) -> Option<Section> {
    plan.tailscale?;
    Some(
        Section::anonymous("rule")
            .with("name", "Allow-Tailscale")
            .with("src", wan)
            .with("proto", "udp")
            .with("dest_port", PORT.to_string())
            .with("target", "ACCEPT"),
    )
}
