//! The mesh backhaul (features/mesh.md, D-013): batman-adv's `bat0` in the `network` package,
//! a bridge trunk like the Ethernet ports, and an 802.11s mesh point as its hard interface in
//! `wireless`. Wired links stay plain trunks with Bridge Loop Avoidance on; batman over wired
//! hard interfaces and backhaul selection come with the managed mesh (M5.3).

use uniwrt_model::{node::NodeRole, site::wireless::MeshRouting};
use uniwrt_uci::Section;

use crate::{
    ext::SectionExt,
    plan::{BAT, Plan},
};

/// The interface, and the wifi-iface, of the mesh point.
/// The mesh point's interface, in `network` and `wireless`.
pub const MESH_IFACE: &str = "uw_mesh";

pub(crate) fn interfaces(plan: &Plan<'_>) -> Vec<Section> {
    if plan.mesh.is_none() {
        return Vec::new();
    }
    let routing = match plan.site.mesh.routing {
        MeshRouting::BatmanIv => "BATMAN_IV",
        MeshRouting::BatmanV => "BATMAN_V",
    };
    // One gateway: it serves, and the APs need no gateway selection.
    let gw_mode = if plan.role == NodeRole::Gateway {
        "server"
    } else {
        "off"
    };
    // batman isolates per VLAN; the untagged network's setting is the mesh's own.
    let isolated = plan
        .nets
        .iter()
        .any(|net| !net.tagged() && net.network.isolation);
    vec![
        Section::named("interface", BAT)
            .with("proto", "batadv")
            .with("routing_algo", routing)
            .with("bridge_loop_avoidance", "1")
            .with("distributed_arp_table", "1")
            .with("multicast_mode", "1")
            .with("orig_interval", "1000")
            .with("aggregated_ogms", "1")
            .with("fragmentation", "1")
            .with("gw_mode", gw_mode)
            .flag("ap_isolation", isolated),
        Section::named("interface", MESH_IFACE)
            .with("proto", "batadv_hardif")
            .with("master", BAT)
            // batman's header needs 32 bytes above the 1500 the bridge carries.
            .with("mtu", "1532"),
    ]
}

/// The 802.11s mesh point on the mesh radio.
pub(crate) fn wifi_iface(plan: &Plan<'_>) -> Option<Section> {
    let mesh = plan.mesh.as_ref()?;
    Some(
        Section::named("wifi-iface", MESH_IFACE)
            .with("device", mesh.radio.name.as_str())
            .with("network", MESH_IFACE)
            .with("mode", "mesh")
            .with("mesh_id", plan.site.mesh.mesh_id.as_str())
            .with("encryption", "sae")
            .with("key", mesh.key.as_str())
            // batman routes; 802.11s only links neighbours.
            .with("mesh_fwding", "0")
            .with("mesh_rssi_threshold", "-80"),
    )
}
