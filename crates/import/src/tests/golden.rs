//! Render's golden sites (`crates/render/tests/support`): what uniwrt rendered imports exactly,
//! with nothing raw and no residual, on every capability set.

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
        AP, GATEWAY, defaults_site, gateway_facts, home_site, pppoe_site, secrets, stock,
        swconfig_facts, triband_facts, x86_facts,
    },
};
use crate::Fate;

fn round_trip(site: &SiteConfig, nodes: &[(NodeId, NodeRole, NodeFacts)]) -> crate::Import {
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
        let exact = node.residual.is_empty()
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
