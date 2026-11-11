//! The simulated fleet's hand-made OpenWrt configs (`uniwrt_sim::brownfield`): a gateway with
//! its LAN, the WAN on VLAN 201 and DHCP, and access points bridging to it over batman-adv,
//! with one SSID on both bands.

use insta::assert_snapshot;
use uniwrt_core::{NodeId, Timestamp};
use uniwrt_model::{
    node::NodeRole,
    site::{
        SiteConfig,
        gateway::{DhcpClient, WanIpv6, WanProto},
        network::Purpose,
        wireless::{NodeScope, WlanSecurity},
    },
    wifi::Band,
};
use uniwrt_sim::{MESH_ID, MESH_KEY, PASSPHRASE, SSID, SimConfig};

use super::{Found, assert_lossless, rendered, run, summary};

fn fleet() -> Vec<Found> {
    let start = Timestamp::from_unix_ms(1_793_245_539_000);
    uniwrt_sim::brownfield(&SimConfig::default(), start)
        .into_iter()
        .zip(1..)
        .map(|(node, id)| Found {
            node: NodeId::new(id),
            role: node.role,
            facts: node.facts,
            packages: node
                .files
                .iter()
                .map(|(name, text)| (name.clone(), uniwrt_uci::parse(name, text).unwrap()))
                .collect(),
        })
        .collect()
}

#[test]
fn the_simulated_fleet_imports_as_it_is() {
    let fleet = fleet();
    assert_eq!(fleet[0].role, NodeRole::Gateway);
    let import = run(&SiteConfig::defaults(None), &fleet);
    assert_lossless(&import);
    let site = &import.site;
    let [lan] = site.networks.as_slice() else {
        panic!("one network: {:?}", site.networks)
    };
    assert_eq!(
        (lan.id.as_str(), lan.purpose, lan.vlan),
        ("lan", Purpose::Default, None)
    );
    assert_eq!(
        lan.ipv4.as_ref().unwrap().address.to_string(),
        "192.168.1.1/24"
    );
    assert_eq!(site.addressing.ipv4.to_string(), "192.168.0.0/16");
    assert_eq!(
        (site.wan.proto.clone(), site.wan.vlan, site.wan.ipv6.clone()),
        (
            WanProto::Dhcp(DhcpClient::default()),
            Some(201),
            WanIpv6::Dhcpv6 { reqprefix: None }
        )
    );
    let [wlan] = site.wlans.as_slice() else {
        panic!("one WLAN: {:?}", site.wlans)
    };
    assert_eq!(wlan.ssid, SSID);
    assert_eq!(wlan.security, WlanSecurity::Wpa2Wpa3);
    assert_eq!(wlan.bands, [Band::G2, Band::G5]);
    assert_eq!(wlan.nodes, NodeScope::All);
    assert!(site.mesh.enabled);
    assert_eq!(
        (site.mesh.mesh_id.as_str(), site.mesh.band),
        (MESH_ID, Some(Band::G5))
    );
    let values: Vec<&str> = import.secrets.values().map(String::as_str).collect();
    assert!(
        values.contains(&PASSPHRASE) && values.contains(&MESH_KEY),
        "{values:?}"
    );
    assert_eq!(site.site.country.as_deref(), Some("US"));
    assert_snapshot!("sim_fleet", summary(&import));
}

#[test]
fn an_import_renders_back_into_itself() {
    let fleet = fleet();
    let base = SiteConfig::defaults(None);
    let first = run(&base, &fleet);
    let again: Vec<Found> = fleet
        .iter()
        .map(|found| Found {
            node: found.node,
            role: found.role,
            facts: found.facts.clone(),
            packages: rendered(&first, found),
        })
        .collect();
    let second = run(&base, &again);
    for (a, b) in first.site.nodes.iter().zip(&second.site.nodes) {
        let show = |n: &uniwrt_model::site::node::NodeConfig| -> Vec<String> {
            n.raw
                .iter()
                .map(|r| {
                    format!(
                        "{}.{}.{:?} m={} {:?}",
                        r.package, r.kind, r.name, r.merge, r.options
                    )
                })
                .collect()
        };
        assert_eq!(show(a), show(b), "{}", a.node);
    }
    let changes = uniwrt_model::site::diff::diff(&first.site, &second.site);
    assert!(changes.is_empty(), "the same site: {changes:#?}");
    assert_eq!(second.secrets, first.secrets);
    for node in &second.nodes {
        assert!(
            node.residual.iter().all(super::ft_keys_only),
            "{}: {:?}",
            node.node,
            node.residual
        );
    }
}
