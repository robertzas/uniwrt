//! The parsers against captured and canonical agent output (`fixtures/collect/`), plus the
//! edge cases that real nodes produce.

#![allow(clippy::needless_pass_by_value)] // `json!` literals read better passed by value

use serde_json::{Value, json, value::RawValue};
use uniwrt_core::Timestamp;
use uniwrt_model::facts::{
    Counters, DeviceFacts, DeviceKind, NodeFacts, PackageManager, Software, SwitchKind, WifiFeature,
};

use super::{FactsError, SetFacts, apply};
use crate::proto::ops::{Collected, Set};

/// Captured from OpenWrt rootfs containers booted with procd (sanitized: host model, CPU and
/// kernel), and the raw output for the canonical model fixture `gateway.json`.
const CONTAINERS: [(&str, &str); 2] = [
    (
        "24.10.8",
        include_str!("../../fixtures/collect/x86-64-24.10.8.json"),
    ),
    (
        "25.12.6",
        include_str!("../../fixtures/collect/x86-64-25.12.6.json"),
    ),
];
const GATEWAY: &str = include_str!("../../fixtures/collect/gateway.json");
const GATEWAY_FACTS: &str = include_str!("../../../model/fixtures/facts/gateway.json");

fn facts_of(json: &str) -> NodeFacts {
    let collected: Collected = serde_json::from_str(json).unwrap();
    let mut facts = NodeFacts::default();
    let errors = apply(&collected, &mut facts);
    assert_eq!(errors, []);
    facts
}

fn parse(set: Set, raw: Value) -> Result<SetFacts, FactsError> {
    SetFacts::parse(set, &RawValue::from_string(raw.to_string()).unwrap())
}

fn device(raw: Value) -> DeviceFacts {
    let Ok(SetFacts::Devices(mut devices)) =
        parse(Set::Devices, json!({"sys": {"dev": raw}, "net_dev": null}))
    else {
        panic!("devices");
    };
    devices.remove(0)
}

/// Parsing the agent's output for the canonical gateway gives the canonical facts: every part
/// that the M1.3 sets cover.
#[test]
fn the_gateway_fixture_parses_into_the_canonical_facts() {
    let parsed = facts_of(GATEWAY);
    let mut canonical: NodeFacts = serde_json::from_str(GATEWAY_FACTS).unwrap();
    canonical.devices.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(parsed.board, canonical.board);
    assert_eq!(parsed.system, canonical.system);
    assert_eq!(parsed.capabilities, canonical.capabilities);
    assert_eq!(parsed.interfaces, canonical.interfaces);
    assert_eq!(parsed.devices, canonical.devices);
}

#[test]
fn container_captures_parse() {
    for (release, json) in CONTAINERS {
        let facts = facts_of(json);
        assert_eq!(facts.board.release.version, release);
        assert_eq!(facts.board.release.arch, "x86_64");
        assert_eq!(facts.board.release.target, "x86/64");
        assert_eq!(facts.board.hostname, "OpenWrt");
        assert!(facts.system.memory.total_bytes > facts.system.memory.available_bytes);
        assert!(facts.system.node_time.is_some());
        assert!(facts.system.cpu_percent.is_some(), "{release}");
        let lan = &facts.interfaces[0];
        assert_eq!(
            (lan.name.as_str(), lan.device.as_deref()),
            ("lan", Some("br-lan"))
        );
        assert_eq!(lan.ipv4[0].to_string(), "192.168.1.1/24");
        assert_eq!(lan.ipv6.len(), 1, "the ULA assignment");
        let kinds: Vec<_> = facts
            .devices
            .iter()
            .map(|d| (d.name.as_str(), d.kind))
            .collect();
        assert_eq!(
            kinds,
            [
                ("br-lan", DeviceKind::Bridge),
                ("eth0", DeviceKind::Other), // a veth
                ("lo", DeviceKind::Loopback),
            ]
        );
        assert_eq!(facts.devices[1].master.as_deref(), Some("br-lan"));
        assert!(facts.devices[1].counters.rx_bytes > 0);
        let expected = if release == "24.10.8" {
            PackageManager::Opkg
        } else {
            PackageManager::Apk
        };
        assert_eq!(facts.capabilities.package_manager, Some(expected));
        assert_eq!(facts.capabilities.switch, None);
        assert!(facts.capabilities.wifi.is_empty(), "no hostapd");
        assert!(facts.capabilities.software.contains(&Software::Luci));
    }
}

#[test]
fn failed_and_malformed_sets_are_reported_and_leave_their_part_alone() {
    let collected: Collected = serde_json::from_value(json!({
        "sets": {"system": {"info": "not an object"}, "interfaces": {"dump": {"interface": []}}},
        "errors": {"board": {"code": "failed", "msg": "ubus system board: Not found"}},
    }))
    .unwrap();
    let mut facts = NodeFacts::default();
    facts.board.hostname = "kept".to_owned();
    let errors = apply(&collected, &mut facts);
    assert_eq!(facts.board.hostname, "kept");
    let sets: Vec<_> = errors.iter().map(|err| err.set).collect();
    assert_eq!(sets, [Set::Board, Set::System]);
    assert_eq!(
        errors[0].to_string(),
        "collect set board: failed on the node: failed: ubus system board: Not found"
    );
}

#[test]
fn device_kinds() {
    let kind = |raw: Value| device(raw).kind;
    assert_eq!(
        kind(json!({"type": "1", "physical": true})),
        DeviceKind::Ethernet
    );
    assert_eq!(kind(json!({"type": "1"})), DeviceKind::Other);
    assert_eq!(
        kind(json!({"type": "1", "uevent": "DEVTYPE=dsa"})),
        DeviceKind::Ethernet
    );
    assert_eq!(
        kind(json!({"type": "1", "uevent": "DEVTYPE=bridge\nINTERFACE=br-lan"})),
        DeviceKind::Bridge
    );
    assert_eq!(
        kind(json!({"type": "1", "uevent": "DEVTYPE=vlan"})),
        DeviceKind::Vlan
    );
    assert_eq!(
        kind(json!({"type": "1", "wireless": true})),
        DeviceKind::Wireless
    );
    assert_eq!(
        kind(json!({"type": "1", "uevent": "DEVTYPE=wlan"})),
        DeviceKind::Wireless
    );
    assert_eq!(
        kind(json!({"type": "1", "uevent": "DEVTYPE=batman"})),
        DeviceKind::Batman
    );
    assert_eq!(
        kind(json!({"type": "65534", "uevent": "DEVTYPE=wireguard"})),
        DeviceKind::Tunnel
    );
    assert_eq!(kind(json!({"type": "778"})), DeviceKind::Tunnel);
    assert_eq!(
        kind(json!({"type": "512", "uevent": "DEVTYPE=ppp"})),
        DeviceKind::Ppp
    );
    assert_eq!(kind(json!({"type": "772"})), DeviceKind::Loopback);
    assert_eq!(kind(json!({})), DeviceKind::Other);
}

#[test]
fn device_link_attributes() {
    let port = device(json!({
        "type": "1", "physical": true, "address": "94:83:C4:00:00:10", "mtu": "1500",
        "flags": "0x1003", "carrier": "1", "speed": "2500", "duplex": "full",
    }));
    assert_eq!(port.mac.unwrap().to_string(), "94:83:c4:00:00:10");
    assert!(port.up && port.carrier);
    assert_eq!(
        (port.speed_mbps, port.full_duplex),
        (Some(2500), Some(true))
    );

    // Down: the kernel refuses to read carrier and speed.
    let down = device(json!({"type": "1", "physical": true, "flags": "0x1002", "speed": null}));
    assert!(!down.up && !down.carrier);
    assert_eq!((down.speed_mbps, down.full_duplex), (None, None));
    // No link: speed -1, duplex unknown.
    let unplugged = device(
        json!({"type": "1", "physical": true, "carrier": "0", "speed": "-1", "duplex": "unknown"}),
    );
    assert_eq!((unplugged.speed_mbps, unplugged.full_duplex), (None, None));
    // A bridge reports its ports' speed, which is not its own.
    let bridge =
        device(json!({"type": "1", "uevent": "DEVTYPE=bridge", "speed": "1000", "duplex": "full"}));
    assert_eq!((bridge.speed_mbps, bridge.full_duplex), (None, None));
    // Loopback and tunnels have no MAC.
    assert_eq!(
        device(json!({"type": "772", "address": "00:00:00:00:00:00"})).mac,
        None
    );
}

#[test]
fn proc_net_dev_counters() {
    let Ok(SetFacts::Devices(devices)) = parse(
        Set::Devices,
        json!({
            "sys": {"eth1.201": {"type": "1"}, "gone": {"type": "1"}},
            "net_dev": "Inter-| head\n face | head\neth1.201:43890221056 32998113 1 2 0 0 0 9 3380117504 10230977 3 4 0 0 0 0\n  junk: 1 2\n",
        }),
    ) else {
        panic!("devices");
    };
    let counters = devices[0].counters;
    assert_eq!(
        (counters.rx_bytes, counters.rx_packets),
        (43_890_221_056, 32_998_113)
    );
    assert_eq!((counters.rx_errors, counters.rx_dropped), (1, 2));
    assert_eq!(
        (counters.tx_bytes, counters.tx_packets),
        (3_380_117_504, 10_230_977)
    );
    assert_eq!((counters.tx_errors, counters.tx_dropped), (3, 4));
    assert_eq!(
        devices[1].counters,
        Counters::default(),
        "missing from /proc/net/dev"
    );
}

#[test]
fn an_interface_that_is_down() {
    let Ok(SetFacts::Interfaces(interfaces)) = parse(
        Set::Interfaces,
        json!({"dump": {"interface": [{
            "interface": "wan", "up": false, "pending": false, "available": false,
            "proto": "pppoe", "device": "wan", "errors": [{"subsystem": "interface", "code": "NO_DEVICE"}],
            "ipv4-address": [{"address": "not an address", "mask": 24}],
        }]}}),
    ) else {
        panic!("interfaces");
    };
    let wan = &interfaces[0];
    assert!(!wan.up);
    assert_eq!((wan.device.as_deref(), wan.uptime_s), (Some("wan"), None));
    assert!(wan.ipv4.is_empty(), "{:?}", wan.ipv4);
    assert_eq!(wan.errors, ["NO_DEVICE"]);
}

#[test]
fn system_values_are_scaled_and_rounded() {
    let Ok(SetFacts::System(system)) = parse(
        Set::System,
        json!({
            "info": {"uptime": 5, "load": [65536, 98304, 0], "memory": {"total": 100}},
            "clock": [1_792_828_799, 999_999_999],
            "cpu_percent": 100.00001,
            "thermal": [{"type": "a", "temp": 48_123}, {"type": "b", "temp": 200_000}],
        }),
    ) else {
        panic!("system");
    };
    assert_eq!(system.load, [1.0, 1.5, 0.0]);
    assert_eq!(
        system.node_time,
        Some(Timestamp::from_unix_ms(1_792_828_799_999))
    );
    assert_eq!(system.cpu_percent, Some(100.0));
    assert_eq!(
        system.temperature_c,
        Some(48.1),
        "the 200 °C sensor is broken"
    );
    assert_eq!((system.root, system.tmp), (None, None));
}

#[test]
fn capabilities_from_a_wired_swconfig_node() {
    let Ok(SetFacts::Capabilities(caps)) = parse(
        Set::Capabilities,
        json!({"package_managers": ["opkg"], "software": ["iw", "something-new"],
               "hostapd": null, "wpa_supplicant": ["sae", "mesh"], "swconfig": true, "dsa": true}),
    ) else {
        panic!("capabilities");
    };
    assert_eq!(caps.package_manager, Some(PackageManager::Opkg));
    assert_eq!(caps.switch, Some(SwitchKind::Swconfig));
    assert_eq!(
        caps.wifi.into_iter().collect::<Vec<_>>(),
        [WifiFeature::Sae, WifiFeature::Mesh],
        "no hostapd: no 11k/v/w"
    );
    assert_eq!(
        caps.software.into_iter().collect::<Vec<_>>(),
        [Software::Iw]
    );
}

#[test]
fn board_falls_back_to_the_release_file() {
    let Ok(SetFacts::Board(board)) = parse(
        Set::Board,
        json!({"board": {"hostname": "ap1"},
               "release": {"DISTRIB_ID": "OpenWrt", "DISTRIB_RELEASE": "24.10.8", "DISTRIB_ARCH": "mipsel_24kc"},
               "board_json": {"system": {"label_macaddr": "bogus"}}}),
    ) else {
        panic!("board");
    };
    assert_eq!(board.release.version, "24.10.8");
    assert_eq!(board.release.arch, "mipsel_24kc");
    assert_eq!(board.label_mac, None);
}
