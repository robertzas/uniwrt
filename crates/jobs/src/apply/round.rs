//! The gateway's transactions within one apply (D-081; features/lifecycle.md,
//! Make-before-break), planned (pure): the first becomes the bridge when the network this
//! controller sits on moves (the old address beside the new one, short leases, the ports to
//! bounce), and leaves the WAN out when it changes among other things; a [`Round`] names each
//! transaction that follows.

use std::collections::BTreeMap;

use uniwrt_model::{job::StepName, site::node::ManagedPackage};
use uniwrt_node::txn::current::ServiceState;
use uniwrt_render::without_wan;
use uniwrt_uci::Package;

use super::address::Bridge;

/// A transaction after a node's first one in an apply, each planned afresh from what the node
/// holds then (D-081).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Round {
    /// Drops the old address the bridge kept, once the controller reaches the new one.
    Settle,
    /// The WAN's change, deferred from the transactions before.
    Wan,
}

impl Round {
    pub(crate) fn step(self) -> StepName {
        match self {
            Self::Settle => StepName::Settle,
            Self::Wan => StepName::Wan,
        }
    }

    /// The suffix of its transaction's id.
    pub(crate) fn suffix(self) -> &'static str {
        match self {
            Self::Settle => "settle",
            Self::Wan => "wan",
        }
    }

    /// What failed, when it does.
    pub(crate) fn failed_words(self) -> &'static str {
        match self {
            Self::Settle => "Dropping the old address",
            Self::Wan => "The WAN's change",
        }
    }
}

/// How long the bridge's leases last: dnsmasq's shortest, so a rollback's hosts return soon.
const BRIDGE_LEASE: &str = "2m";

/// The packages that stand on the WAN and change with it, last.
pub(super) const WAN_PACKAGES: [ManagedPackage; 2] = [ManagedPackage::Sqm, ManagedPackage::Mwan3];

fn changed(before: Option<&Package>, after: Option<&Package>, name: &str) -> bool {
    let empty = Package::new(name);
    !uniwrt_uci::diff(before.unwrap_or(&empty), after.unwrap_or(&empty)).is_empty()
}

/// Whether the gateway's WAN changes among other things, so it goes last (D-081): the WAN is
/// `network`'s WAN sections (`uniwrt_render::wan_sections`) and the packages that stand on it.
pub(super) fn wan_last(
    rendered: &BTreeMap<ManagedPackage, Package>,
    current: &BTreeMap<String, Package>,
    wanted: &BTreeMap<&'static str, bool>,
    services: &BTreeMap<String, ServiceState>,
) -> bool {
    let Some(network) = rendered.get(&ManagedPackage::Network) else {
        return false;
    };
    let empty = Package::new("network");
    let now = current.get("network").unwrap_or(&empty);
    let rest = without_wan(network, now);
    let wan_changes = !uniwrt_uci::diff(&rest, network).is_empty()
        || WAN_PACKAGES
            .iter()
            .any(|p| changed(current.get(p.name()), rendered.get(p), p.name()));
    let service_changes = |keep: &dyn Fn(&str) -> bool| {
        wanted.iter().any(|(name, on)| {
            keep(name)
                && match services.get(*name) {
                    Some(ServiceState::On) => !on,
                    _ => *on,
                }
        })
    };
    let wan_service = |name: &str| WAN_PACKAGES.iter().any(|p| p.name() == name);
    let other_changes = !uniwrt_uci::diff(now, &rest).is_empty()
        || rendered.iter().any(|(p, after)| {
            *p != ManagedPackage::Network
                && !WAN_PACKAGES.contains(p)
                && changed(current.get(p.name()), Some(after), p.name())
        })
        || service_changes(&|name| !wan_service(name));
    wan_changes && other_changes
}

/// Makes this transaction the bridge: the renumbered network's interface keeps the old address
/// beside the new one, and its pool leases for [`BRIDGE_LEASE`].
pub(super) fn bridge_round(
    bridge: &Bridge,
    rendered: &BTreeMap<ManagedPackage, Package>,
    round: &mut BTreeMap<ManagedPackage, Package>,
) {
    if let Some(network) = round.get_mut(&ManagedPackage::Network)
        && let Some(section) = network.section_mut(&bridge.network)
    {
        section.set(
            "ipaddr",
            vec![bridge.new.to_string(), bridge.old.to_string()],
        );
    }
    if rendered.contains_key(&ManagedPackage::Dhcp)
        && let Some(dhcp) = round.get_mut(&ManagedPackage::Dhcp)
        && let Some(section) = dhcp.section_mut(&bridge.network)
        && section.get("leasetime").is_some()
    {
        section.set("leasetime", BRIDGE_LEASE);
    }
}

/// The bridge's Ethernet ports, which the hosts hang off: not the mesh, not the WAN.
pub(super) fn bridge_ports(rendered: &BTreeMap<ManagedPackage, Package>) -> Vec<String> {
    rendered
        .get(&ManagedPackage::Network)
        .into_iter()
        .flat_map(|network| network.sections_of("device"))
        .filter(|section| section.option("name") == Some(uniwrt_render::BRIDGE))
        .filter_map(|section| section.get("ports").map(uniwrt_uci::Value::items))
        .flatten()
        .filter(|port| *port != uniwrt_render::BAT && !port.starts_with("wan"))
        .cloned()
        .collect()
}

#[cfg(test)]
#[path = "round_tests.rs"]
mod tests;
