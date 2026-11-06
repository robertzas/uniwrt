//! Golden tests (docs/dev/TESTING.md): each role × feature × capability set renders into a
//! snapshot under `tests/snapshots/`. Review every change to one (`cargo insta review`).

// Tests and their fixtures may panic on what they set up.
#![allow(clippy::unwrap_used)]

mod support;

use insta::assert_snapshot;
use uniwrt_model::{
    facts::{Software, WifiFeature},
    node::NodeRole,
    site::{
        gateway::{BackupLink, BackupProto, DhcpClient, Failover, Sqm, WanIpv6, WanProto},
        node::{ManagedPackage, RawSection, RawValue},
        wireless::{Channel, WlanSecurity},
    },
    wifi::Band,
};
use uniwrt_render::Rendered;

use support::*;

fn ok(result: Result<Rendered, Vec<uniwrt_render::Issue>>) -> Rendered {
    result.unwrap_or_else(|issues| panic!("{issues:?}"))
}

#[test]
fn gateway_defaults() {
    let result = run(
        &defaults_site(),
        GATEWAY,
        NodeRole::Gateway,
        &gateway_facts(),
    );
    assert_snapshot!("gateway_defaults", show(&result));
}

#[test]
fn ap_defaults() {
    let result = run(&defaults_site(), AP, NodeRole::Ap, &gateway_facts());
    assert_snapshot!("ap_defaults", show(&result));
}

#[test]
fn gateway_dsa_home() {
    let result = run(&home_site(), GATEWAY, NodeRole::Gateway, &gateway_facts());
    assert_snapshot!("gateway_dsa_home", show(&result));
}

#[test]
fn ap_dsa_home() {
    let result = run(&home_site(), AP, NodeRole::Ap, &gateway_facts());
    assert_snapshot!("ap_dsa_home", show(&result));
}

#[test]
fn ap_triband_dedicates_the_high_band_to_the_mesh() {
    let mut site = home_site();
    for policy in &mut site.radio.bands {
        if policy.band == Band::G5 {
            policy.channel = Channel::Fixed(149);
        }
    }
    let result = run(&site, AP, NodeRole::Ap, &triband_facts());
    let rendered = ok(result.clone());
    let wireless = &rendered.packages[&ManagedPackage::Wireless];
    let mesh = wireless.section("uw_mesh").unwrap();
    assert_eq!(mesh.option("device"), Some("radio2"));
    assert!(
        wireless.sections.iter().all(|s| s.kind != "wifi-iface"
            || s.name.as_deref() == Some("uw_mesh")
            || s.option("device") != Some("radio2")),
        "no WLAN on the mesh's radio"
    );
    assert_snapshot!("ap_triband_mesh", show(&result));
}

#[test]
fn gateway_swconfig_pppoe() {
    let result = run(&pppoe_site(), GATEWAY, NodeRole::Gateway, &swconfig_facts());
    assert_snapshot!("gateway_swconfig_pppoe", show(&result));
}

#[test]
fn ap_swconfig() {
    let result = run(&pppoe_site(), AP, NodeRole::Ap, &swconfig_facts());
    assert_snapshot!("ap_swconfig", show(&result));
}

#[test]
fn gateway_x86_static() {
    let mut site = defaults_site();
    site.site.timezone = "Europe/Berlin".to_owned();
    site.wan.proto = WanProto::Static {
        address: "203.0.113.10/29".parse().unwrap(),
        gateway: "203.0.113.9".parse().unwrap(),
        dns: vec!["9.9.9.9".parse().unwrap(), "2620:fe::fe".parse().unwrap()],
    };
    site.wan.ipv6 = WanIpv6::Off;
    let result = run(&site, GATEWAY, NodeRole::Gateway, &x86_facts());
    assert_snapshot!("gateway_x86_static", show(&result));
}

#[test]
fn gateway_isp_vlan_6rd() {
    let mut site = defaults_site();
    site.wan.vlan = Some(201);
    site.wan.pcp = Some(1);
    site.wan.mtu = Some(1500);
    site.wan.proto = WanProto::Dhcp(DhcpClient {
        vendor_class: Some("uniwrt".to_owned()),
        client_id: Some("01020304".to_owned()),
        send_options: vec!["77:4d4f44454d".to_owned(), "90:00ff".to_owned()],
        hostname: Some("router".to_owned()),
    });
    site.wan.ipv6 = WanIpv6::SixRd {
        prefix: "2602::/24".parse().unwrap(),
        border_relay: "205.171.2.64".parse().unwrap(),
        ipv4_mask_len: 0,
    };
    let result = run(&site, GATEWAY, NodeRole::Gateway, &gateway_facts());
    assert_snapshot!("gateway_isp_vlan_6rd", show(&result));
}

#[test]
fn gateway_dslite_and_ipv6_only_wans() {
    let mut site = defaults_site();
    site.wan.ipv6 = WanIpv6::Dhcpv6 {
        reqprefix: Some(56),
    };
    let port = gateway_facts().board.layout.wan.unwrap();
    let mut wans = String::new();
    for proto in [
        WanProto::DsLite { aftr: None },
        WanProto::DsLite {
            aftr: Some("aftr.example.net".to_owned()),
        },
        WanProto::None,
    ] {
        site.wan.proto = proto;
        site.wan.mtu = Some(1460);
        let packages = ok(run(&site, GATEWAY, NodeRole::Gateway, &gateway_facts())).packages;
        let wan = uniwrt_uci::Package {
            name: "network".to_owned(),
            sections: packages[&ManagedPackage::Network]
                .sections
                .iter()
                .filter(|s| {
                    s.name.as_deref().is_some_and(|n| n.starts_with("wan"))
                        || (s.kind == "device" && s.option("name") == Some(port.as_str()))
                })
                .cloned()
                .collect(),
        };
        wans.push_str(&wan.to_file());
        wans.push_str("---\n");
    }
    assert_snapshot!("gateway_dslite_and_ipv6_only_wans", wans);
}

#[test]
fn rendering_is_deterministic_and_valid() {
    let site = home_site();
    for (role, node, facts) in [
        (NodeRole::Gateway, GATEWAY, gateway_facts()),
        (NodeRole::Ap, AP, gateway_facts()),
        (NodeRole::Ap, AP, triband_facts()),
    ] {
        let a = ok(run(&site, node, role, &facts));
        let b = ok(run(&site, node, role, &facts));
        assert_eq!(a, b);
        for package in a.packages.values() {
            package.validate().unwrap();
            let back = uniwrt_uci::parse(&package.name, &package.to_file()).unwrap();
            assert_eq!(&back, package, "{} reads back as itself", package.name);
        }
    }
}

#[test]
fn a_port_a_raw_interface_takes_leaves_the_bridge() {
    let mut site = home_site();
    let facts = gateway_facts();
    let port = facts.board.layout.wan.clone().unwrap();
    let ap = site.nodes.iter_mut().find(|n| n.node == AP).unwrap();
    ap.raw.push(RawSection {
        package: "network".to_owned(),
        kind: "interface".to_owned(),
        name: Some("batwired".to_owned()),
        options: [
            ("proto", "batadv_hardif"),
            ("master", "bat0"),
            ("device", port.as_str()),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), RawValue::Option(v.to_owned())))
        .collect(),
        merge: false,
    });
    let rendered = ok(run(&site, AP, NodeRole::Ap, &facts));
    let network = &rendered.packages[&ManagedPackage::Network];
    let bridge = network
        .sections_of("device")
        .find(|d| d.option("name") == Some("br-lan"))
        .unwrap();
    let ports = bridge.get("ports").unwrap().items();
    assert!(!ports.contains(&port), "{ports:?}");
    assert!(ports.iter().any(|p| p == "bat0"));
    assert!(
        network.sections_of("bridge-vlan").all(|v| v
            .get("ports")
            .unwrap()
            .items()
            .iter()
            .all(|p| !p.starts_with(&format!("{port}:")))),
        "nor its VLANs"
    );
    assert_eq!(
        network.section("batwired").unwrap().option("device"),
        Some(port.as_str())
    );
}

#[test]
fn only_managed_packages_render() {
    let mut site = home_site();
    site.nodes[1].managed = [ManagedPackage::Wireless].into();
    let rendered = ok(run(&site, AP, NodeRole::Ap, &gateway_facts()));
    assert_eq!(
        rendered.packages.keys().copied().collect::<Vec<_>>(),
        [ManagedPackage::Wireless]
    );
    assert!(rendered.services.is_empty());
    site.nodes.clear();
    let rendered = ok(run(&site, AP, NodeRole::Ap, &gateway_facts()));
    assert!(
        rendered.packages.is_empty(),
        "an unmanaged node gets nothing"
    );
}

#[test]
fn what_keeps_a_node_from_rendering() {
    let errors = |result: Result<Rendered, Vec<uniwrt_render::Issue>>| -> Vec<String> {
        result
            .expect_err("errors")
            .into_iter()
            .map(|issue| issue.path)
            .collect()
    };
    // A package without a renderer yet.
    let mut site = defaults_site();
    site.nodes[0].managed.insert(ManagedPackage::Dropbear);
    assert_eq!(
        errors(run(&site, GATEWAY, NodeRole::Gateway, &gateway_facts())),
        ["node"]
    );
    // The mesh without batman-adv or 802.11s.
    let mut facts = gateway_facts();
    facts.capabilities.software.remove(&Software::BatmanAdv);
    assert_eq!(
        errors(run(&home_site(), AP, NodeRole::Ap, &facts)),
        ["mesh"]
    );
    // A gateway without a WAN port.
    let mut facts = x86_facts();
    facts.board.layout.wan = None;
    assert_eq!(
        errors(run(&defaults_site(), GATEWAY, NodeRole::Gateway, &facts)),
        ["wan"]
    );
    // A secret the vault doesn't have.
    let mut site = home_site();
    site.wlans[0].passphrase = Some(secret(9));
    assert!(
        errors(run(&site, AP, NodeRole::Ap, &gateway_facts()))
            .contains(&"wlans/home/passphrase".to_owned())
    );
    // WPA3 on a hostapd without SAE.
    let mut site = pppoe_site();
    site.wlans[0].security = WlanSecurity::Wpa3;
    let mut facts = swconfig_facts();
    facts.capabilities.wifi.remove(&WifiFeature::Sae);
    assert!(errors(run(&site, AP, NodeRole::Ap, &facts)).contains(&"wlans/home".to_owned()));
    // A fixed channel the radio doesn't have.
    let mut site = defaults_site();
    site.radio.bands[1].channel = Channel::Fixed(165);
    site.wlans = home_site().wlans;
    site.wlans.retain(|w| w.passphrase.is_none());
    assert_eq!(
        errors(run(&site, AP, NodeRole::Ap, &gateway_facts())),
        ["node/radios/radio1"]
    );
}

#[test]
fn transition_mode_falls_back_to_wpa2_without_sae() {
    let mut facts = swconfig_facts();
    facts.capabilities.wifi.remove(&WifiFeature::Sae);
    let rendered = ok(run(&pppoe_site(), AP, NodeRole::Ap, &facts));
    let wireless = &rendered.packages[&ManagedPackage::Wireless];
    assert_eq!(
        wireless.section("uw_home_2g").unwrap().option("encryption"),
        Some("psk2")
    );
    assert!(rendered.warnings.iter().any(|w| w.path == "wlans/home"));
}

fn sqm(overhead: Option<u8>) -> Sqm {
    Sqm {
        download_kbps: 850_000,
        upload_kbps: 90_000,
        overhead,
        autotune: false,
    }
}

#[test]
fn gateway_sqm_and_failover_on_a_lan_port() {
    let facts = gateway_facts();
    let port = "lan4".to_owned();
    let mut site = home_site();
    site.wan.sqm = Some(sqm(Some(44)));
    site.wan.failover = Some(Failover {
        link: BackupLink::Port {
            port: port.clone(),
            vlan: Some(300),
        },
        proto: BackupProto::Static {
            address: "192.0.2.10/24".parse().unwrap(),
            gateway: "192.0.2.1".parse().unwrap(),
            dns: vec!["192.0.2.1".parse().unwrap()],
        },
    });
    let result = run(&site, GATEWAY, NodeRole::Gateway, &facts);
    let rendered = ok(result.clone());
    let network = &rendered.packages[&ManagedPackage::Network];
    let bridge = network
        .sections_of("device")
        .find(|d| d.option("name") == Some("br-lan"))
        .unwrap();
    assert!(
        !bridge.get("ports").unwrap().items().contains(&port),
        "the backup's port leaves the bridge"
    );
    assert_snapshot!("gateway_sqm_and_failover_on_a_lan_port", show(&result));
}

#[test]
fn gateway_pppoe_sqm_and_usb_failover() {
    let mut site = pppoe_site();
    site.wan.sqm = Some(sqm(None));
    site.wan.failover = Some(Failover {
        link: BackupLink::Usb {
            device: "usb0".to_owned(),
        },
        proto: BackupProto::Dhcp,
    });
    let result = run(&site, GATEWAY, NodeRole::Gateway, &gateway_facts());
    assert_snapshot!("gateway_pppoe_sqm_and_usb_failover", show(&result));
}

#[test]
fn sqm_and_failover_off_leave_what_the_gateway_holds_inert() {
    let site = home_site();
    let mut current = stock();
    for (name, text) in [
        (
            "sqm",
            "config queue 'eth1'\n\toption enabled '1'\n\toption interface 'eth1'\n",
        ),
        (
            "mwan3",
            "config globals 'globals'\n\toption mmx_mask '0x3F00'\n\nconfig interface 'wan'\n\toption enabled '1'\n",
        ),
    ] {
        current.insert(name.to_owned(), uniwrt_uci::parse(name, text).unwrap());
    }
    let rendered = ok(run_on(
        &site,
        GATEWAY,
        NodeRole::Gateway,
        &gateway_facts(),
        &current,
    ));
    assert_eq!(rendered.packages[&ManagedPackage::Sqm].sections, []);
    let mwan3 = &rendered.packages[&ManagedPackage::Mwan3];
    assert_eq!(
        mwan3
            .sections
            .iter()
            .map(|s| s.kind.as_str())
            .collect::<Vec<_>>(),
        ["globals"]
    );
    assert_eq!(rendered.services.get("sqm"), Some(&false));
    assert_eq!(rendered.services.get("mwan3"), Some(&false));
    assert_eq!(rendered.requires, []);
    // Off and not on the node: nothing to own.
    let rendered = ok(run(&site, GATEWAY, NodeRole::Gateway, &gateway_facts()));
    assert!(!rendered.packages.contains_key(&ManagedPackage::Sqm));
    assert!(!rendered.services.contains_key("mwan3"));
    // Never on an access point.
    let mut site = home_site();
    site.wan.sqm = Some(sqm(None));
    let rendered = ok(run_on(&site, AP, NodeRole::Ap, &gateway_facts(), &current));
    assert!(!rendered.packages.contains_key(&ManagedPackage::Sqm));
}

#[test]
fn a_backup_needs_a_port_of_its_own() {
    let mut site = home_site();
    let facts = gateway_facts();
    site.wan.failover = Some(Failover {
        link: BackupLink::Port {
            port: facts.board.layout.wan.clone().unwrap(),
            vlan: None,
        },
        proto: BackupProto::Dhcp,
    });
    let issues = run(&site, GATEWAY, NodeRole::Gateway, &facts).unwrap_err();
    assert!(
        issues.iter().any(|i| i.path == "wan/failover/link"),
        "{issues:?}"
    );
    let facts = swconfig_facts();
    site.wan.failover = Some(Failover {
        link: BackupLink::Port {
            port: facts.board.layout.lan[0].clone(),
            vlan: None,
        },
        proto: BackupProto::Dhcp,
    });
    let issues = run(&site, GATEWAY, NodeRole::Gateway, &facts).unwrap_err();
    assert!(
        issues.iter().any(|i| i.message.contains("swconfig")),
        "{issues:?}"
    );
}
