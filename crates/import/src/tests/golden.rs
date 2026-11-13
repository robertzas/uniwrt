//! Render's golden sites (`crates/render/tests/support`): what uniwrt rendered imports exactly,
//! with nothing raw and no residual but the 802.11r keys (the controller's replace a node's
//! own, D-085), on every capability set.

use uniwrt_core::NodeId;
use uniwrt_model::{
    facts::NodeFacts,
    node::NodeRole,
    site::{
        SiteConfig,
        gateway::{DhcpClient, WanIpv6, WanProto},
        wireless::Channel,
    },
    wifi::Band,
};
use uniwrt_render::Target;

use super::{
    Found, assert_lossless, run, summary,
    support::{
        AP, GATEWAY, defaults_site, gateway_facts, home_site, lan_site, pppoe_site, secrets, stock,
        swconfig_facts, triband_facts, x86_facts,
    },
};
use crate::Fate;

fn round_trip(site: &SiteConfig, nodes: &[(NodeId, NodeRole, NodeFacts)]) -> crate::Import {
    round_trip_from(site, nodes, None)
}

/// [`round_trip`] of an apply made from `controller`.
fn round_trip_from(
    site: &SiteConfig,
    nodes: &[(NodeId, NodeRole, NodeFacts)],
    controller: Option<std::net::IpAddr>,
) -> crate::Import {
    let current = stock();
    let found: Vec<Found> = nodes
        .iter()
        .map(|(node, role, facts)| {
            let rendered = uniwrt_render::render(
                site,
                &secrets(),
                Target {
                    node: *node,
                    role: *role,
                    facts,
                    current: &current,
                    controller,
                    backhaul: &[],
                },
            )
            .expect("the golden site renders");
            Found {
                node: *node,
                role: *role,
                facts: facts.clone(),
                packages: rendered
                    .packages
                    .into_values()
                    .map(|p| (p.name.clone(), p))
                    .collect(),
            }
        })
        .collect();
    let import = run(&SiteConfig::defaults(None), &found);
    assert_lossless(&import);
    for node in &import.nodes {
        let exact = node.residual.iter().all(super::ft_keys_only)
            && node
                .sections
                .iter()
                .all(|s| matches!(&s.fate, Fate::Mapped { kept } if kept.is_empty()));
        assert!(exact, "imports exactly:\n{}", summary(&import));
        assert_eq!(node.confidence, 100);
    }
    import
}

#[test]
fn the_defaults_import_exactly() {
    round_trip(
        &defaults_site(),
        &[
            (GATEWAY, NodeRole::Gateway, gateway_facts()),
            (AP, NodeRole::Ap, gateway_facts()),
        ],
    );
}

#[test]
fn the_home_site_imports_exactly() {
    round_trip(
        &home_site(),
        &[
            (GATEWAY, NodeRole::Gateway, gateway_facts()),
            (AP, NodeRole::Ap, gateway_facts()),
        ],
    );
}

#[test]
fn a_triband_mesh_imports_exactly() {
    let mut site = home_site();
    for policy in &mut site.radio.bands {
        if policy.band == Band::G5 {
            policy.channel = Channel::Fixed(149);
        }
    }
    round_trip(
        &site,
        &[
            (GATEWAY, NodeRole::Gateway, gateway_facts()),
            (AP, NodeRole::Ap, triband_facts()),
        ],
    );
}

#[test]
fn swconfig_and_pppoe_import_exactly() {
    round_trip(
        &pppoe_site(),
        &[
            (GATEWAY, NodeRole::Gateway, swconfig_facts()),
            (AP, NodeRole::Ap, swconfig_facts()),
        ],
    );
}

#[test]
fn lan_services_and_port_profiles_import_exactly() {
    let site = lan_site(false);
    let import = round_trip_from(
        &site,
        &[
            (GATEWAY, NodeRole::Gateway, gateway_facts()),
            (AP, NodeRole::Ap, gateway_facts()),
        ],
        Some("10.0.0.2".parse().unwrap()),
    );
    let pools = |site: &SiteConfig| -> Vec<_> {
        site.networks
            .iter()
            .map(|n| (n.ipv6_clients, n.ipv4.clone().and_then(|v4| v4.dhcp)))
            .collect()
    };
    assert_eq!(pools(&import.site), pools(&site));
    // The controller's record is render's, not a custom one.
    assert_eq!(import.site.services.dns_records, site.services.dns_records);
    for node in [GATEWAY, AP] {
        let ports = |site: &SiteConfig| {
            site.nodes
                .iter()
                .find(|n| n.node == node)
                .map(|n| n.ports.clone())
        };
        assert_eq!(ports(&import.site), ports(&site), "node {node}");
    }
}

#[test]
fn swconfig_port_profiles_import_exactly() {
    let mut site = lan_site(true);
    site.mesh.enabled = false;
    let import = round_trip(
        &site,
        &[
            (GATEWAY, NodeRole::Gateway, swconfig_facts()),
            (AP, NodeRole::Ap, swconfig_facts()),
        ],
    );
    for node in &site.nodes {
        let imported = import
            .site
            .nodes
            .iter()
            .find(|n| n.node == node.node)
            .unwrap();
        assert_eq!(imported.ports, node.ports, "node {}", node.node);
    }
}

#[test]
fn a_pc_router_imports_exactly() {
    round_trip(
        &defaults_site(),
        &[(GATEWAY, NodeRole::Gateway, x86_facts())],
    );
}

#[test]
fn every_wan_imports_exactly() {
    let sixrd = WanIpv6::SixRd {
        prefix: "2602::/24".parse().unwrap(),
        border_relay: "205.171.2.64".parse().unwrap(),
        ipv4_mask_len: 0,
    };
    let dhcpv6 = WanIpv6::Dhcpv6 {
        reqprefix: Some(56),
    };
    let dhcp = WanProto::Dhcp(DhcpClient {
        vendor_class: Some("uniwrt".to_owned()),
        client_id: Some("0102".to_owned()),
        send_options: vec!["77:4d4f44454d".to_owned(), "90:00ff".to_owned()],
        hostname: Some("*".to_owned()),
    });
    let pppoe = pppoe_site().wan.proto;
    let pppoe = match pppoe {
        WanProto::Pppoe {
            username, password, ..
        } => WanProto::Pppoe {
            username,
            password,
            service: Some("fiber".to_owned()),
        },
        other => other,
    };
    let cases = [
        (dhcp, Some(201), Some(5), Some(1500), sixrd.clone()),
        (pppoe.clone(), Some(7), None, Some(1480), dhcpv6.clone()),
        (pppoe.clone(), None, None, None, sixrd),
        (pppoe, None, None, None, WanIpv6::Off),
        (
            WanProto::DsLite { aftr: None },
            None,
            None,
            Some(1460),
            dhcpv6.clone(),
        ),
        (
            WanProto::DsLite {
                aftr: Some("aftr.example.net".to_owned()),
            },
            Some(201),
            None,
            None,
            WanIpv6::Dhcpv6 { reqprefix: None },
        ),
        (WanProto::None, None, None, Some(1400), dhcpv6),
    ];
    for (proto, vlan, pcp, mtu, ipv6) in cases {
        let mut site = defaults_site();
        site.wan.proto = proto;
        site.wan.vlan = vlan;
        site.wan.pcp = pcp;
        site.wan.mtu = mtu;
        site.wan.ipv6 = ipv6;
        let import = round_trip(&site, &[(GATEWAY, NodeRole::Gateway, gateway_facts())]);
        let mut wan = import.site.wan;
        if let (WanProto::Pppoe { password: a, .. }, WanProto::Pppoe { password: b, .. }) =
            (&mut wan.proto, &site.wan.proto)
        {
            a.clone_from(b);
        }
        assert_eq!(wan, site.wan);
    }
}

#[test]
fn sqm_and_failover_import_exactly() {
    use uniwrt_model::site::gateway::{BackupLink, BackupProto, Failover, Sqm};
    let port = Failover {
        link: BackupLink::Port {
            port: "lan4".to_owned(),
            vlan: Some(300),
        },
        proto: BackupProto::Static {
            address: "192.0.2.10/24".parse().unwrap(),
            gateway: "192.0.2.1".parse().unwrap(),
            dns: vec!["192.0.2.1".parse().unwrap()],
        },
    };
    let usb = Failover {
        link: BackupLink::Usb {
            device: "usb0".to_owned(),
        },
        proto: BackupProto::Dhcp,
    };
    let plain = Failover {
        link: BackupLink::Port {
            port: "lan3".to_owned(),
            vlan: None,
        },
        proto: BackupProto::Dhcp,
    };
    let sqm = |overhead| Sqm {
        download_kbps: 850_000,
        upload_kbps: 90_000,
        overhead,
        autotune: false,
    };
    let cases = [
        (home_site(), Some(sqm(Some(44))), Some(port)),
        (pppoe_site(), Some(sqm(None)), Some(usb)),
        (home_site(), None, Some(plain)),
        (home_site(), Some(sqm(Some(18))), None),
    ];
    for (mut site, sqm, failover) in cases {
        site.wan.sqm = sqm;
        site.wan.failover = failover;
        let import = round_trip(&site, &[(GATEWAY, NodeRole::Gateway, gateway_facts())]);
        assert_eq!(import.site.wan.sqm, site.wan.sqm);
        assert_eq!(import.site.wan.failover, site.wan.failover);
    }
}

#[test]
fn the_adoption_port_imports_exactly() {
    use uniwrt_model::site::adoption::AdoptionPort;
    for adoption in [
        AdoptionPort::new("lan2"),
        AdoptionPort {
            port: "lan1".to_owned(),
            address: "192.168.1.2".parse().unwrap(),
        },
    ] {
        let mut site = home_site();
        site.adoption_port = Some(adoption);
        let import = round_trip(
            &site,
            &[
                (GATEWAY, NodeRole::Gateway, gateway_facts()),
                (AP, NodeRole::Ap, gateway_facts()),
            ],
        );
        assert_eq!(import.site.adoption_port, site.adoption_port);
        let ids = |site: &SiteConfig| {
            site.networks
                .iter()
                .map(|n| n.id.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&import.site), ids(&site), "no network of its own");
    }
}

#[test]
fn a_queue_set_up_by_hand_is_renamed() {
    let mut site = home_site();
    site.wan.sqm = None;
    let facts = gateway_facts();
    let current = stock();
    let rendered = uniwrt_render::render(
        &site,
        &secrets(),
        Target {
            node: GATEWAY,
            role: NodeRole::Gateway,
            facts: &facts,
            current: &current,
            controller: None,
            backhaul: &[],
        },
    )
    .unwrap();
    let mut packages: std::collections::BTreeMap<String, uniwrt_uci::Package> = rendered
        .packages
        .into_values()
        .map(|p| (p.name.clone(), p))
        .collect();
    let sqm = "config queue 'eth1'\n\toption enabled '1'\n\toption interface 'eth1.201'\n\
               \toption download '50000'\n\toption upload '10000'\n\toption qdisc 'cake'\n\
               \toption script 'piece_of_cake.qos'\n\toption linklayer 'none'\n";
    packages.insert("sqm".to_owned(), uniwrt_uci::parse("sqm", sqm).unwrap());
    let found = [Found {
        node: GATEWAY,
        role: NodeRole::Gateway,
        facts,
        packages,
    }];
    let import = run(&SiteConfig::defaults(None), &found);
    let sqm = import.site.wan.sqm.as_ref().expect("the queue imports");
    assert_eq!((sqm.download_kbps, sqm.upload_kbps), (50_000, 10_000));
    let node = &import.nodes[0];
    assert!(
        node.sections.iter().all(|s| !matches!(s.fate, Fate::Raw)),
        "nothing kept whole:\n{}",
        summary(&import)
    );
}
