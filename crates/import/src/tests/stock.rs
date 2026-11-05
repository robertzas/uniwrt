//! OpenWrt's stock configs (`crates/uci/fixtures/openwrt/`): 24.10's and 25.12's on a PC
//! router, and the hwsim lab's gateway with its radios.

use insta::assert_snapshot;
use uniwrt_model::{
    node::NodeRole,
    site::{SiteConfig, node::RawValue},
};

use super::{
    Found, assert_lossless, parse, rendered, run, summary,
    support::{GATEWAY, gateway_facts, x86_facts},
};

macro_rules! release {
    ($release:literal) => {
        parse(&[
            (
                "system",
                include_str!(concat!(
                    "../../../uci/fixtures/openwrt/",
                    $release,
                    "/system"
                )),
            ),
            (
                "network",
                include_str!(concat!(
                    "../../../uci/fixtures/openwrt/",
                    $release,
                    "/network"
                )),
            ),
            (
                "dhcp",
                include_str!(concat!("../../../uci/fixtures/openwrt/", $release, "/dhcp")),
            ),
            (
                "firewall",
                include_str!(concat!(
                    "../../../uci/fixtures/openwrt/",
                    $release,
                    "/firewall"
                )),
            ),
        ])
    };
}

fn stock_gateway(packages: std::collections::BTreeMap<String, uniwrt_uci::Package>) -> String {
    let found = [Found {
        node: GATEWAY,
        role: NodeRole::Gateway,
        facts: x86_facts(),
        packages,
    }];
    let import = run(&SiteConfig::defaults(None), &found);
    assert_lossless(&import);
    let lan = &import.site.networks[0];
    assert_eq!(
        lan.ipv4.as_ref().unwrap().address.to_string(),
        "192.168.1.1/24"
    );
    summary(&import)
}

#[test]
fn openwrt_24_10_imports() {
    assert_snapshot!("stock_24_10", stock_gateway(release!("24.10")));
}

#[test]
fn openwrt_25_12_imports() {
    assert_snapshot!("stock_25_12", stock_gateway(release!("25.12")));
}

#[test]
fn the_hwsim_gateway_imports() {
    let mut packages = release!("25.12");
    packages.extend(parse(&[
        (
            "network",
            include_str!("../../../uci/fixtures/openwrt/gateway/network"),
        ),
        (
            "wireless",
            include_str!("../../../uci/fixtures/openwrt/gateway/wireless"),
        ),
        (
            "dhcp",
            include_str!("../../../uci/fixtures/openwrt/gateway/dhcp"),
        ),
        (
            "firewall",
            include_str!("../../../uci/fixtures/openwrt/gateway/firewall"),
        ),
    ]));
    // The fixture is a hand-made gateway whose WAN port is `wan` (DSA).
    let mut facts = gateway_facts();
    facts.board.layout.wan = Some("wan".to_owned());
    let found = [Found {
        node: GATEWAY,
        role: NodeRole::Gateway,
        facts,
        packages,
    }];
    let import = run(&SiteConfig::defaults(None), &found);
    assert_lossless(&import);
    assert_snapshot!("hwsim_gateway", summary(&import));
}

#[test]
fn raw_secrets_are_sealed_and_render_back() {
    let mut packages = release!("25.12");
    let wireguard = "
config interface 'wg0'
\toption proto 'wireguard'
\toption private_key 'cHJpdmF0ZQ=='
\tlist addresses '10.9.0.1/24'

config wireguard_wg0
\toption public_key 'cHVibGlj'
\toption preshared_key 'cHJlc2hhcmVk'
\tlist allowed_ips '10.9.0.2/32'
";
    let network = format!(
        "{}{wireguard}",
        include_str!("../../../uci/fixtures/openwrt/25.12/network")
    );
    packages.extend(parse(&[("network", &network)]));
    let found = [Found {
        node: GATEWAY,
        role: NodeRole::Gateway,
        facts: x86_facts(),
        packages,
    }];
    let import = run(&SiteConfig::defaults(None), &found);
    assert_lossless(&import);
    let raw = &import.site.nodes[0].raw;
    let sealed: Vec<_> = raw
        .iter()
        .flat_map(|r| &r.options)
        .filter_map(|(name, value)| match value {
            RawValue::Secret { secret } => Some((name.as_str(), import.secrets[secret].as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(
        sealed,
        [
            ("private_key", "cHJpdmF0ZQ=="),
            ("preshared_key", "cHJlc2hhcmVk")
        ]
    );
    assert!(
        raw.iter()
            .flat_map(|r| r.options.values())
            .all(|v| !matches!(v, RawValue::Option(v) if v.contains("cHJ"))),
        "no secret stays in plain text"
    );
    let network = &rendered(&import, &found[0])["network"];
    let wg0 = network
        .sections
        .iter()
        .find(|s| s.name.as_deref() == Some("wg0"));
    assert_eq!(wg0.unwrap().option("private_key"), Some("cHJpdmF0ZQ=="));
    let paths: Vec<_> = import
        .site
        .secrets()
        .into_iter()
        .filter(|slot| slot.path.starts_with("nodes/"))
        .map(|slot| slot.path)
        .collect();
    assert_eq!(paths.len(), 2, "{paths:?}");
}
