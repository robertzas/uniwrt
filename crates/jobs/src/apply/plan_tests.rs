//! Planning a node's part from what it reports, on the facts of OpenWrt 24.10 in a container.

use std::collections::{BTreeMap, BTreeSet};

use uniwrt_core::{NodeId, Timestamp};
use uniwrt_model::{
    apply::{MASKED, PlanAction},
    facts::NodeFacts,
    node::{Node, NodeMode, NodeRole},
    site::{
        SecretRef, SiteConfig,
        node::{ManagedPackage, NodeConfig, RawSection, RawValue},
    },
};
use uniwrt_node::txn::current::ServiceState;
use uniwrt_render::RENDERABLE;

use super::{Part, Report, plan};
use crate::ApplyTiming;
use crate::testing::COLLECTED;

/// What the agent collects on OpenWrt 24.10 in a container; a gateway's board.json gets a WAN
/// port (`eth1`), which the container lacks.
pub(crate) fn collected(role: NodeRole) -> serde_json::Value {
    let mut collected: serde_json::Value = serde_json::from_str(COLLECTED).unwrap();
    if role == NodeRole::Gateway {
        collected["sets"]["board"]["board_json"]["network"]["wan"] =
            serde_json::json!({"device": "eth1", "protocol": "dhcp"});
    }
    collected
}

pub(crate) fn facts_of(role: NodeRole) -> NodeFacts {
    let collected = serde_json::from_value(collected(role)).unwrap();
    let mut facts = NodeFacts::default();
    assert_eq!(uniwrt_node::facts::apply(&collected, &mut facts), []);
    facts
}

pub(crate) fn node(id: i64, role: NodeRole, host: &str) -> Node {
    Node {
        id: NodeId::new(id),
        name: format!("node-{id}"),
        mac: uniwrt_core::MacAddr::new([2, 0, 0, 0, 0, u8::try_from(id).unwrap()]),
        board_name: "container".to_owned(),
        host: host.to_owned(),
        port: 22,
        role,
        mode: NodeMode::Managed,
        created_at: Timestamp::from_unix_ms(0),
        updated_at: Timestamp::from_unix_ms(0),
    }
}

/// The defaults with the LAN on the stock 192.168.1.0/24, UPnP off (a new site's is on, which
/// would install miniupnpd first), and `nodes` managing every package render writes.
pub(crate) fn site(nodes: &[NodeId]) -> SiteConfig {
    let mut site = SiteConfig::defaults(None);
    site.networks[0].ipv4.as_mut().unwrap().address = "192.168.1.1/24".parse().unwrap();
    let dhcp = site.networks[0]
        .ipv4
        .as_mut()
        .unwrap()
        .dhcp
        .as_mut()
        .unwrap();
    dhcp.start = "192.168.1.100".parse().unwrap();
    dhcp.end = "192.168.1.249".parse().unwrap();
    site.firewall.upnp.enabled = false;
    site.nodes = nodes
        .iter()
        .map(|&id| NodeConfig {
            managed: RENDERABLE.into_iter().collect::<BTreeSet<ManagedPackage>>(),
            ..NodeConfig::new(id)
        })
        .collect();
    site
}

fn stock_services() -> BTreeMap<String, ServiceState> {
    ["dnsmasq", "odhcpd", "firewall"]
        .into_iter()
        .map(|name| (name.to_owned(), ServiceState::On))
        .collect()
}

fn part(site: &SiteConfig, node: &Node, files: &BTreeMap<String, Option<String>>) -> Part {
    let facts = facts_of(node.role);
    let secrets: BTreeMap<SecretRef, String> = BTreeMap::new();
    plan(
        site,
        &secrets,
        &Report {
            node,
            facts: &facts,
            files,
            services: &stock_services(),
            installed: Ok(&BTreeSet::new()),
            controller: None,
        },
        &ApplyTiming::default(),
    )
}

#[test]
fn a_new_node_gets_every_package_and_then_holds_them() {
    let ap = node(2, NodeRole::Ap, "192.168.1.20");
    let site = site(&[ap.id]);
    let first = part(&site, &ap, &BTreeMap::new());
    assert_eq!(
        first.plan.action,
        PlanAction::Change,
        "{:?}",
        first.plan.issues
    );
    let packages: Vec<&str> = first.files.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(packages, ["system", "network", "dhcp", "firewall"]);
    // An AP serves no DHCP and runs no firewall.
    assert_eq!(
        first.services,
        [
            ("dnsmasq".to_owned(), false),
            ("firewall".to_owned(), false),
            ("odhcpd".to_owned(), false)
        ]
    );
    assert_eq!(
        (first.address, first.timeout),
        (None, ApplyTiming::default().timer)
    );

    // Holding what it rendered, with the services off, nothing changes.
    let files = first
        .files
        .iter()
        .map(|(name, file)| (name.clone(), Some(file.clone())))
        .collect();
    let facts = facts_of(NodeRole::Ap);
    let off = ["dnsmasq", "odhcpd", "firewall"]
        .into_iter()
        .map(|name| (name.to_owned(), ServiceState::Off))
        .collect();
    let secrets: BTreeMap<SecretRef, String> = BTreeMap::new();
    let again = plan(
        &site,
        &secrets,
        &Report {
            node: &ap,
            facts: &facts,
            files: &files,
            services: &off,
            installed: Ok(&BTreeSet::new()),
            controller: None,
        },
        &ApplyTiming::default(),
    );
    assert_eq!(again.plan.action, PlanAction::Unchanged);
    assert!(again.files.is_empty() && again.plan.packages.is_empty());
}

#[test]
fn a_node_managing_nothing_is_unchanged_and_bad_files_refuse_it() {
    let gw = node(1, NodeRole::Gateway, "192.168.1.1");
    let unmanaged = part(&site(&[]), &gw, &BTreeMap::new());
    assert_eq!(
        unmanaged.plan.action,
        PlanAction::Unchanged,
        "{:?}",
        unmanaged.plan.issues
    );
    let broken = BTreeMap::from([("network".to_owned(), Some("config 'x\n".to_owned()))]);
    let refused = part(&site(&[gw.id]), &gw, &broken);
    assert_eq!(refused.plan.action, PlanAction::Refused);
    assert!(
        refused.plan.issues[0]
            .message
            .contains("/etc/config/network")
    );
}

#[test]
fn moving_the_gateway_is_planned_and_an_ap_without_a_reservation_refused() {
    let gw = node(1, NodeRole::Gateway, "192.168.1.1");
    let ap = node(2, NodeRole::Ap, "192.168.1.20");
    let mut site = site(&[gw.id, ap.id]);
    site.networks[0].ipv4.as_mut().unwrap().address = "10.0.0.1/24".parse().unwrap();
    let dhcp = site.networks[0]
        .ipv4
        .as_mut()
        .unwrap()
        .dhcp
        .as_mut()
        .unwrap();
    dhcp.start = "10.0.0.100".parse().unwrap();
    dhcp.end = "10.0.0.249".parse().unwrap();
    let moved = part(&site, &gw, &BTreeMap::new());
    assert_eq!(moved.address, Some("10.0.0.1".parse().unwrap()));
    let change = moved.plan.address.unwrap();
    assert_eq!(
        (change.from.as_str(), change.to.as_str()),
        ("192.168.1.1", "10.0.0.1")
    );
    let refused = part(&site, &ap, &BTreeMap::new());
    assert_eq!(refused.plan.action, PlanAction::Refused);
    assert!(refused.plan.issues[0].message.contains("reserve one"));
}

#[test]
fn secrets_are_masked_in_the_plan() {
    let package = |key: &str| {
        uniwrt_uci::parse(
            "wireless",
            &format!("config wifi-iface 'home'\n\toption ssid 'Home'\n\toption key '{key}'\n"),
        )
        .unwrap()
    };
    let mut diff = uniwrt_uci::diff(&package("old secret"), &package("new secret"));
    super::mask(&mut diff);
    let text = serde_json::to_string(&diff).unwrap();
    assert!(!text.contains("secret"), "{text}");
    let option = &diff.sections[0].options[0];
    assert_eq!(option.name, "key");
    assert_eq!(
        option.after,
        Some(uniwrt_uci::Value::Single(MASKED.to_owned()))
    );
}

#[test]
fn the_plan_shows_the_raw_overrides_of_the_packages_it_writes() {
    let gw = node(1, NodeRole::Gateway, "192.168.1.1");
    let raw = |package: &str, name: &str| RawSection {
        package: package.to_owned(),
        kind: "interface".to_owned(),
        name: Some(name.to_owned()),
        options: [("proto".to_owned(), RawValue::Option("none".to_owned()))].into(),
        merge: false,
    };
    let mut site = site(&[gw.id]);
    site.nodes[0].raw = vec![raw("network", "spare"), raw("dropbear", "x")];
    let planned = part(&site, &gw, &BTreeMap::new());
    assert_eq!(planned.plan.raw, vec![raw("network", "spare")]);
    site.nodes[0].managed.remove(&ManagedPackage::Network);
    assert_eq!(part(&site, &gw, &BTreeMap::new()).plan.raw, Vec::new());
}

#[test]
fn a_gateway_with_sqm_installs_its_packages_first() {
    use uniwrt_model::{
        facts::PackageManager,
        site::gateway::{BackupLink, BackupProto, Failover, Sqm},
    };
    let gw = node(1, NodeRole::Gateway, "192.168.1.1");
    let mut site = site(&[gw.id]);
    site.wan.sqm = Some(Sqm {
        download_kbps: 100_000,
        upload_kbps: 20_000,
        overhead: None,
        autotune: false,
    });
    site.wan.failover = Some(Failover {
        link: BackupLink::Usb {
            device: "usb0".to_owned(),
        },
        proto: BackupProto::Dhcp,
    });
    site.firewall.upnp.enabled = true;
    let mut facts = facts_of(NodeRole::Gateway);
    facts.capabilities.package_manager = Some(PackageManager::Opkg);
    let secrets: BTreeMap<SecretRef, String> = BTreeMap::new();
    let planned = |installed: &BTreeSet<String>, facts: &uniwrt_model::facts::NodeFacts| {
        plan(
            &site,
            &secrets,
            &Report {
                node: &gw,
                facts,
                files: &BTreeMap::new(),
                services: &stock_services(),
                installed: Ok(installed),
                controller: None,
            },
            &ApplyTiming::default(),
        )
    };
    let first = planned(&BTreeSet::new(), &facts);
    assert_eq!(
        first.plan.action,
        PlanAction::Change,
        "{:?}",
        first.plan.issues
    );
    assert_eq!(
        first.plan.installs,
        [
            "sqm-scripts",
            "kmod-sched-cake",
            "mwan3",
            "kmod-usb-net-rndis",
            "kmod-usb-net-cdc-ether",
            "miniupnpd-nftables"
        ]
    );
    let (manager, _) = first.installs.as_ref().unwrap();
    assert_eq!(*manager, PackageManager::Opkg);
    // Their services come with them, and the transaction switches them on.
    assert!(first.services.contains(&("sqm".to_owned(), true)));
    assert!(first.services.contains(&("mwan3".to_owned(), true)));
    assert!(first.services.contains(&("miniupnpd".to_owned(), true)));
    // Installed already: nothing to install; the services, absent, are an issue no more.
    let all: BTreeSet<String> = first.plan.installs.iter().cloned().collect();
    let again = planned(&all, &facts);
    assert!(again.installs.is_none() && again.plan.installs.is_empty());
    // Without a package manager uniwrt knows, it can't apply.
    facts.capabilities.package_manager = None;
    let refused = planned(&BTreeSet::new(), &facts);
    assert_eq!(refused.plan.action, PlanAction::Refused);
}
