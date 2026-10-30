//! Every rendered set and push, through the node crate's parsers: what a simulated node reports
//! must read as a real one's does, and agree with the world.

use std::sync::Arc;

use serde_json::{Value, value::to_raw_value};
use uniwrt_core::Timestamp;
use uniwrt_model::{
    event::{LeaseChange, NodeEvent, StationChange},
    facts::{DeviceKind, NodeFacts},
};
use uniwrt_node::{
    AgentEvent,
    events::{self, Push},
    facts,
    proto::ops::{Collected, Set},
};

use super::{collect, log_entries, log_lines, notification};
use crate::{
    SimConfig,
    world::{What, World},
};

const START: Timestamp = Timestamp::from_unix_ms(1_793_245_539_000);

fn world() -> World {
    World::new(&SimConfig::default(), START)
}

fn facts_of(world: &World, node: usize) -> NodeFacts {
    let collected: Collected =
        serde_json::from_value(collect(world, node, &Set::ALL)).expect("a collect answer");
    let mut facts = NodeFacts::default();
    let errors = facts::apply(&collected, &mut facts);
    assert!(errors.is_empty(), "node {node}: {errors:?}");
    facts
}

#[test]
fn every_set_of_every_node_parses() {
    let mut world = world();
    for _ in 0..3 {
        for node in 0..world.nodes.len() {
            facts_of(&world, node);
        }
        for _ in 0..600 {
            world.step();
        }
    }
}

#[test]
fn the_facts_agree_with_the_world() {
    let world = world();
    let all: Vec<_> = (0..world.nodes.len())
        .map(|n| facts_of(&world, n))
        .collect();
    let gateway = &all[0];
    assert_eq!(gateway.board.hostname, "gateway");
    assert_eq!(gateway.board.model, "GL.iNet GL-MT6000");
    assert_eq!(all[3].board.release.version, "24.10.8");
    assert!(gateway.system.uptime_s > 86_400 * 12);

    // Every client at home on Wi-Fi is a station of exactly one node.
    let wireless = world
        .clients
        .iter()
        .filter(|client| client.assoc.is_some())
        .count();
    let stations: usize = all
        .iter()
        .map(|facts| {
            facts
                .stations
                .iter()
                .filter(|s| s.iface.ends_with("-ap0"))
                .count()
        })
        .sum();
    assert_eq!(stations, wireless);
    assert!(wireless >= 25, "{wireless} of 30 clients on Wi-Fi");
    // Each node has a mesh link with at least one other.
    for facts in &all {
        assert!(facts.stations.iter().any(|s| s.iface == "phy1-mesh0"));
        assert_eq!(facts.radios.len(), 2);
        assert_eq!(facts.phys.len(), 2);
        assert!(
            facts
                .radios
                .iter()
                .all(|radio| radio.phy.is_some() && radio.noise_dbm.is_some())
        );
        let batman = &facts.batman[0];
        let mut reached: Vec<_> = batman.originators.iter().map(|o| o.mac).collect();
        reached.dedup();
        assert_eq!(reached.len(), 3, "{batman:?}");
    }
    // The gateway leases to every client and access point, and routes to the ISP.
    assert_eq!(gateway.leases.iter().filter(|l| l.ip.is_ipv4()).count(), 33);
    let wan = gateway
        .interfaces
        .iter()
        .find(|i| i.name == "wan")
        .expect("wan");
    assert_eq!(wan.device.as_deref(), Some("eth1.201"));
    assert!(gateway.neighbors.len() > 20);
    assert!(
        gateway
            .devices
            .iter()
            .any(|d| d.name == "br-lan" && d.counters.rx_bytes > 0)
    );
    let kind = |name: &str| {
        gateway
            .devices
            .iter()
            .find(|d| d.name == name)
            .map(|d| d.kind)
    };
    assert_eq!(kind("lan1"), Some(DeviceKind::Ethernet));
    assert_eq!(kind("eth1.201"), Some(DeviceKind::Vlan));
    assert_eq!(kind("br-lan"), Some(DeviceKind::Bridge));
    assert_eq!(kind("phy1-mesh0"), Some(DeviceKind::Wireless));
    assert_eq!(kind("lo"), Some(DeviceKind::Loopback));
    // An access point reaches the gateway as its batman gateway.
    assert_eq!(all[2].batman[0].gateways.len(), 1);
    assert_eq!(all[1].leases, []);
}

#[test]
fn the_far_access_point_routes_through_a_nearer_one() {
    let world = world();
    let garden = facts_of(&world, 3);
    let gateway_mac = world.nodes[0].mesh_mac();
    let best = garden.batman[0]
        .originators
        .iter()
        .find(|o| o.mac == gateway_mac && o.best)
        .expect("a best route to the gateway");
    assert_ne!(best.next_hop, gateway_mac, "{best:?}");
}

#[test]
fn notifications_and_logs_parse() {
    let mut world = world();
    let (mut stations, mut leases) = (0, 0);
    for _ in 0..1800 {
        for happening in world.step() {
            let Some((_, topic, data)) = notification(&world, &happening) else {
                assert!(matches!(happening.what, What::Down | What::Up));
                continue;
            };
            let event = AgentEvent::Event {
                topic,
                ts: Some(happening.at),
                data: raw(&data),
            };
            match events::parse(&event).expect("a push") {
                Some(Push::Event(NodeEvent::Station(station))) => {
                    stations += 1;
                    if station.change == StationChange::AssocRequest {
                        assert!(station.signal_dbm.is_some() && station.freq_mhz.is_some());
                    }
                }
                Some(Push::Event(NodeEvent::Lease(lease))) => {
                    assert_eq!(lease.change, LeaseChange::Acked);
                    leases += 1;
                }
                other => panic!("unexpected {other:?}"),
            }
        }
    }
    assert!(
        stations > 20 && leases > 2,
        "{stations} station and {leases} lease events"
    );

    let records: Vec<_> = world.nodes[0].logd.records.iter().collect();
    assert!(records.len() > 20);
    for batch in [log_entries(&records, 0), log_lines(&records)] {
        let event = AgentEvent::Event {
            topic: "log".to_owned(),
            ts: None,
            data: raw(&batch),
        };
        let Some(Push::Log { entries, dropped }) = events::parse(&event).expect("a log batch")
        else {
            panic!("not a log batch");
        };
        assert_eq!((entries.len(), dropped), (records.len(), 0));
        assert!(
            entries
                .iter()
                .any(|e| e.program.as_deref() == Some("dnsmasq-dhcp"))
        );
    }
}

fn raw(value: &Value) -> Arc<serde_json::value::RawValue> {
    to_raw_value(value).unwrap().into()
}

#[test]
fn adoption_s_commands_answer_as_on_openwrt() {
    let world = world();
    let run = |node: usize, argv: &[&str], keys: &mut Vec<String>| {
        let argv: Vec<String> = argv.iter().map(|&arg| arg.to_owned()).collect();
        super::commands::exec(&world.nodes[node], &argv, keys)
    };
    let mut keys = Vec::new();
    let release = run(0, &["cat", "/etc/openwrt_release"], &mut keys);
    let release = String::from_utf8(release.stdout).unwrap();
    assert!(release.starts_with("DISTRIB_ID='OpenWrt'\nDISTRIB_RELEASE='"));

    let install = [
        "sh",
        "-c",
        "f=/etc/dropbear/authorized_keys …",
        "sh",
        "ssh-ed25519 AAAA uniwrt",
    ];
    assert_eq!(run(0, &install, &mut keys).stdout, b"added\n");
    assert_eq!(run(0, &install, &mut keys).stdout, b"present\n");
    assert_eq!(keys, ["ssh-ed25519 AAAA uniwrt"]);

    // Each node reads its own package manager's database, and only that one.
    let opkg = [
        "grep",
        "-E",
        "^(Package|Version|Status):",
        "/usr/lib/opkg/status",
    ];
    let apk = ["grep", "-E", "^[PV]:", "/lib/apk/db/installed"];
    for node in 0..world.nodes.len() {
        let (theirs, other) = if world.nodes[node].release.package_manager == "opkg" {
            (&opkg, &apk)
        } else {
            (&apk, &opkg)
        };
        let output = run(node, theirs, &mut keys);
        assert!(output.status.success());
        assert!(
            String::from_utf8(output.stdout)
                .unwrap()
                .contains("dropbear")
        );
        assert!(!run(node, other, &mut keys).status.success());
    }

    let backup = run(0, &["sysupgrade", "-b", "-"], &mut keys);
    assert!(backup.stdout.starts_with(&[0x1f, 0x8b]));
    let missing = run(0, &["reboot"], &mut keys);
    assert_eq!(missing.status, uniwrt_node::ExitStatus::Code(127));
}
