//! The deriver over hand-made statuses, facts, topologies, clients, log entries and jobs.

use std::sync::Arc;

use serde_json::{Value, json};
use uniwrt_clients::Client;
use uniwrt_core::{JobId, LogId, MacAddr, NodeId, Timestamp};
use uniwrt_model::{
    event::{Attachment, Roam},
    events::{EventCategory, EventKind, EventLevel, NewEvent},
    facts::{NodeFacts, Storage},
    job::{Job, JobKind, JobState},
    log::{LogEntry, LogRecord, LogVia, Severity},
    node::NodeRole,
    status::{NodeState, NodeStatus, StatusReason},
};
use uniwrt_topo::Topology;

use crate::{DeriveConfig, Deriver, Seed};

const N1: NodeId = NodeId::new(1);
const N2: NodeId = NodeId::new(2);

const fn at(s: i64) -> Timestamp {
    Timestamp::from_unix_ms(s * 1_000)
}

fn deriver() -> Deriver {
    Deriver::new(DeriveConfig::default(), Seed::default())
}

fn status(state: NodeState, reasons: Vec<StatusReason>) -> NodeStatus {
    NodeStatus {
        state,
        since: at(0),
        reasons,
    }
}

fn set(deriver: &mut Deriver, node: NodeId, state: NodeState, s: i64) -> Vec<NewEvent> {
    deriver.status(node, &status(state, Vec::new()), at(s))
}

fn names(events: &[NewEvent]) -> Vec<&'static str> {
    events.iter().map(|event| event.what.name()).collect()
}

#[test]
fn a_node_offline_for_a_while_raises_and_coming_back_clears_with_the_downtime() {
    let mut d = deriver();
    assert_eq!(set(&mut d, N1, NodeState::Online, 0), Vec::new());
    assert_eq!(set(&mut d, N1, NodeState::Offline, 100), Vec::new());
    assert_eq!(d.tick(at(129)), Vec::new());
    let raised = d.tick(at(130));
    assert_eq!(names(&raised), ["node_offline"]);
    assert_eq!((raised[0].at, raised[0].node), (at(100), Some(N1)));
    assert_eq!(raised[0].level, EventLevel::Critical);
    let condition = raised[0].condition.clone().unwrap();
    assert_eq!(
        (condition.key.as_str(), condition.raised),
        ("node:1/offline", true)
    );
    let back = set(&mut d, N1, NodeState::Online, 200);
    assert_eq!(
        back[0].what,
        EventKind::NodeOnline {
            down_ms: Some(100_000)
        }
    );
    assert!(!back[0].condition.as_ref().unwrap().raised);
    // A short drop raises nothing.
    set(&mut d, N1, NodeState::Offline, 300);
    set(&mut d, N1, NodeState::Online, 310);
    assert_eq!(d.tick(at(400)), Vec::new());
}

#[test]
fn a_node_the_controller_stops_watching_is_not_offline() {
    let mut d = deriver();
    set(&mut d, N1, NodeState::Online, 0);
    d.status(
        N1,
        &status(NodeState::Offline, vec![StatusReason::Stopped]),
        at(10),
    );
    assert_eq!(d.tick(at(100)), Vec::new());
}

#[test]
fn a_degraded_node_raises_after_a_minute() {
    let mut d = deriver();
    set(&mut d, N1, NodeState::Online, 0);
    set(&mut d, N1, NodeState::Degraded, 10);
    assert_eq!(d.tick(at(69)), Vec::new());
    assert_eq!(names(&d.tick(at(70))), ["node_degraded"]);
    let recovered = set(&mut d, N1, NodeState::Online, 100);
    assert_eq!(
        recovered[0].what,
        EventKind::NodeRecovered {
            degraded_ms: Some(90_000)
        }
    );
}

fn facts(collected: i64, uptime_s: u64) -> NodeFacts {
    let mut facts = NodeFacts {
        collected_at: at(collected),
        ..NodeFacts::default()
    };
    facts.system.uptime_s = uptime_s;
    facts
}

#[test]
fn a_reboot_moves_the_boot_time() {
    let mut d = deriver();
    let mut facts_at = |collected, uptime, fresh| {
        d.facts(
            N1,
            NodeRole::Ap,
            &facts(collected, uptime),
            fresh,
            at(collected),
        )
    };
    assert_eq!(facts_at(2_000, 1_000, true), Vec::new());
    // Polling jitter, and an uptime older than `collected_at` (another set was collected).
    assert_eq!(facts_at(2_016, 1_015, true), Vec::new());
    assert_eq!(facts_at(2_500, 1_015, false), Vec::new());
    let rebooted = facts_at(3_000, 30, true);
    assert_eq!(names(&rebooted), ["node_rebooted"]);
    assert_eq!(rebooted[0].at, at(2_970));
    assert_eq!(
        rebooted[0].what,
        EventKind::NodeRebooted { up_s: Some(1_015) }
    );
}

#[test]
fn flash_runs_low_below_a_tenth_and_is_fine_again_from_fifteen_percent() {
    let mut d = deriver();
    let mut with_free = |available_bytes: u64, s| {
        let mut facts = facts(s, 100);
        facts.system.root = Some(Storage {
            total_bytes: 1_000,
            used_bytes: 1_000 - available_bytes,
            available_bytes,
        });
        names(&d.facts(N1, NodeRole::Ap, &facts, true, at(s)))
    };
    assert_eq!(with_free(500, 1), Vec::<&str>::new());
    assert_eq!(with_free(90, 2), ["flash_low"]);
    assert_eq!(with_free(80, 3), Vec::<&str>::new());
    assert_eq!(with_free(120, 4), Vec::<&str>::new());
    assert_eq!(with_free(150, 5), ["flash_ok"]);
}

fn wan(addresses: &[&str], up: bool) -> Value {
    json!({
        "name": "wan", "proto": "dhcp", "up": up, "device": "eth1", "uptime_s": null,
        "ipv4": addresses, "ipv6": [], "delegated_prefixes": [],
        "ipv4_gateway": if up { json!("203.0.113.1") } else { Value::Null },
        "ipv6_gateway": null, "dns_servers": [], "errors": [],
    })
}

fn with_interfaces(s: i64, interfaces: Value) -> NodeFacts {
    let mut facts = facts(s, 100);
    facts.interfaces = serde_json::from_value(interfaces).unwrap();
    facts
}

#[test]
fn a_gateway_s_wan_address_changes() {
    let mut d = deriver();
    let mut wan_at = |s, interfaces, role| {
        names(&d.facts(N1, role, &with_interfaces(s, interfaces), false, at(s)))
    };
    let gateway = NodeRole::Gateway;
    assert_eq!(
        wan_at(1, json!([wan(&["203.0.113.5/24"], true)]), gateway),
        Vec::<&str>::new()
    );
    assert_eq!(
        wan_at(2, json!([wan(&["203.0.113.5/24"], true)]), gateway),
        Vec::<&str>::new()
    );
    // Down a moment (the WAN link's event), then back with the same address: no change.
    assert_eq!(
        wan_at(3, json!([wan(&[], false)]), gateway),
        Vec::<&str>::new()
    );
    assert_eq!(
        wan_at(4, json!([wan(&["203.0.113.5/24"], true)]), gateway),
        Vec::<&str>::new()
    );
    assert_eq!(
        wan_at(5, json!([wan(&["203.0.113.9/24"], true)]), gateway),
        ["wan_address_changed"]
    );
    let mut ap = deriver();
    for (s, address) in [(1, "198.51.100.2/24"), (2, "198.51.100.3/24")] {
        let facts = with_interfaces(s, json!([wan(&[address], true)]));
        assert_eq!(ap.facts(N2, NodeRole::Ap, &facts, false, at(s)), Vec::new());
    }
}

fn vertex(id: &str, node: Option<i64>) -> Value {
    json!({
        "id": id, "kind": if node.is_some() { "ap" } else { "internet" }, "node": node,
        "mac": null, "name": null, "ips": [], "state": null, "cpu_percent": null,
        "memory_percent": null, "parent": null, "uplink": null, "hops": null,
    })
}

fn edge(a: &str, b: &str, medium: &str, up: bool, quality: Option<u8>) -> Value {
    json!({
        "id": format!("{a}~{b}~{medium}"), "a": a, "b": b, "medium": medium,
        "a_port": null, "b_port": if medium == "wan" { json!("eth1") } else { Value::Null },
        "up": up, "band": null, "ssid": null, "signal_dbm": null, "tq": null,
        "capacity_mbps": null, "quality": quality, "traffic": null,
    })
}

fn topology(edges: &[Value]) -> Topology {
    serde_json::from_value(json!({
        "at": 0,
        "nodes": [vertex("internet", None), vertex("node:1", Some(1)), vertex("node:2", Some(2))],
        "edges": edges,
    }))
    .unwrap()
}

#[test]
fn a_wan_link_without_a_default_route_is_down() {
    let mut d = deriver();
    set(&mut d, N1, NodeState::Online, 0);
    let wan = |up| topology(&[edge("internet", "node:1", "wan", up, None)]);
    assert_eq!(d.topology(&wan(true), at(1)), Vec::new());
    assert_eq!(d.topology(&wan(false), at(10)), Vec::new());
    let down = d.tick(at(20));
    assert_eq!(
        down[0].what,
        EventKind::WanDown {
            device: "eth1".into()
        }
    );
    assert_eq!(down[0].condition.as_ref().unwrap().key, "node:1/wan:eth1");
    let up = d.topology(&wan(true), at(70));
    assert_eq!(
        up[0].what,
        EventKind::WanUp {
            device: "eth1".into(),
            down_ms: Some(60_000)
        }
    );
    // While the gateway is offline, its WAN says nothing.
    set(&mut d, N1, NodeState::Offline, 100);
    d.topology(&wan(false), at(101));
    assert!(
        d.tick(at(200))
            .iter()
            .all(|event| event.what.name() == "node_offline")
    );
}

#[test]
fn a_mesh_link_that_vanishes_while_both_nodes_are_online_is_down() {
    let mut d = deriver();
    set(&mut d, N1, NodeState::Online, 0);
    set(&mut d, N2, NodeState::Online, 0);
    let mesh = topology(&[edge("node:1", "node:2", "mesh", true, Some(80))]);
    assert_eq!(d.topology(&mesh, at(1)), Vec::new());
    assert_eq!(d.topology(&topology(&[]), at(10)), Vec::new());
    let down = d.tick(at(40));
    assert_eq!(names(&down), ["mesh_link_down"]);
    assert_eq!(down[0].node, Some(N2));
    let EventKind::MeshLinkDown { peer, .. } = &down[0].what else {
        panic!("{down:?}");
    };
    assert_eq!(*peer, N1);
    let up = d.topology(&mesh, at(100));
    assert_eq!(names(&up), ["mesh_link_up"]);
    // A node going offline takes its links along: its own event says so.
    set(&mut d, N2, NodeState::Offline, 200);
    d.topology(&topology(&[]), at(201));
    assert_eq!(names(&d.tick(at(300))), ["node_offline"]);
}

#[test]
fn a_weak_mesh_link_raises_after_two_minutes_and_recovers_from_fifty() {
    let mut d = deriver();
    set(&mut d, N1, NodeState::Online, 0);
    set(&mut d, N2, NodeState::Online, 0);
    let quality = |q| topology(&[edge("node:1", "node:2", "mesh", true, Some(q))]);
    d.topology(&quality(30), at(0));
    d.topology(&quality(35), at(60));
    assert_eq!(d.tick(at(119)), Vec::new());
    let weak = d.tick(at(120));
    assert_eq!(names(&weak), ["mesh_link_weak"]);
    assert!(matches!(
        weak[0].what,
        EventKind::MeshLinkWeak { quality: 35, .. }
    ));
    assert_eq!(d.topology(&quality(45), at(130)), Vec::new());
    assert_eq!(
        names(&d.topology(&quality(60), at(180))),
        ["mesh_link_recovered"]
    );
}

fn client(mac: &str, online: bool, node: i64, since: i64) -> Arc<Client> {
    Arc::new(
        serde_json::from_value(json!({
            "mac": mac, "alias": null, "hostname": "pixel", "vendor": "Google", "kind": "phone",
            "randomized": false, "group": null, "ips": [], "online": online,
            "connection": {
                "node": node, "upstream": format!("node:{node}"), "upstream_name": null,
                "port": "phy1-ap0", "medium": "wifi", "since": since * 1_000, "band": "5g",
                "ssid": "home", "channel": null, "signal_dbm": null, "noise_dbm": null,
                "rx_rate": null, "tx_rate": null, "standard": null, "capabilities": [],
                "capacity_mbps": null, "traffic": null,
            },
            "first_seen": 0, "last_seen": 0, "experience": null,
            "wifi": {"standard": null, "bands": [], "capabilities": []},
        }))
        .unwrap(),
    )
}

const PHONE: &str = "f0:18:98:00:00:01";

#[test]
fn clients_join_leave_and_are_new_once_their_node_has_settled() {
    let mut d = deriver();
    set(&mut d, N1, NodeState::Online, 0);
    // The node's first station lists show clients that were there all along.
    assert_eq!(d.clients(&[client(PHONE, true, 1, 5)], at(10)), Vec::new());
    let left = d.clients(&[client(PHONE, false, 1, 5)], at(100));
    assert_eq!(
        left[0].what,
        EventKind::ClientLeft {
            connected_ms: Some(95_000)
        }
    );
    assert_eq!(
        (left[0].node, left[0].client),
        (Some(N1), Some(PHONE.parse().unwrap()))
    );
    let joined = d.clients(&[client(PHONE, true, 1, 200)], at(200));
    assert!(matches!(
        &joined[0].what,
        EventKind::ClientJoined { place: Some(place), ssid: Some(ssid), .. }
            if place == "phy1-ap0" && ssid == "home"
    ));
    let laptop = "f0:18:98:00:00:02";
    let new = d.clients(&[client(laptop, true, 1, 300)], at(300));
    assert_eq!(names(&new), ["new_client"]);
    assert_eq!(new[0].level, EventLevel::Notice);
    // Clients don't leave a node that went offline: they vanish with it.
    set(&mut d, N1, NodeState::Offline, 400);
    assert_eq!(
        d.clients(&[client(laptop, false, 1, 300)], at(401)),
        Vec::new()
    );
}

#[test]
fn clients_that_no_node_places_are_new_only_once_the_deriver_has_run_a_minute() {
    let seed = Seed {
        started: at(1_000),
        ..Seed::default()
    };
    let mut d = Deriver::new(DeriveConfig::default(), seed);
    let mut unplaced = |mac: &str, s: i64| {
        let mut view = (*client(mac, true, 1, s)).clone();
        view.connection = None;
        names(&d.clients(&[Arc::new(view)], at(s)))
    };
    assert_eq!(unplaced(PHONE, 1_010), Vec::<&str>::new());
    assert_eq!(unplaced("f0:18:98:00:00:02", 1_070), ["new_client"]);
}

fn record(id: i64, pattern: Option<&str>, received: i64) -> LogRecord {
    LogRecord {
        id: LogId::new(id),
        received: at(received),
        node: Some(N1),
        host: None,
        via: LogVia::Agent,
        entry: LogEntry {
            seq: None,
            time: at(received),
            facility: 3,
            severity: Severity::Warning,
            program: Some("hostapd".into()),
            pid: None,
            message: format!("phy1-ap0: DFS-RADAR-DETECTED {}", "x".repeat(400)),
        },
        pattern: pattern.map(str::to_owned),
    }
}

#[test]
fn log_entries_whose_pattern_is_an_event_are_events() {
    let mut d = deriver();
    let event_of = |pattern: &str| {
        (pattern == "dfs-radar").then_some((EventCategory::Wifi, Severity::Warning))
    };
    let records = [
        record(1, Some("dfs-radar"), 10),
        record(2, None, 11),
        record(3, Some("wifi-inactivity"), 12),
    ];
    let events = d.logged(&records, event_of);
    assert_eq!(events.len(), 1);
    let event = &events[0];
    assert_eq!(
        (event.category, event.level),
        (EventCategory::Wifi, EventLevel::Warning)
    );
    assert_eq!(event.fold.as_deref(), Some("log:dfs-radar:node:1"));
    let EventKind::LogPattern { log, text, .. } = &event.what else {
        panic!("{event:?}");
    };
    assert_eq!(*log, LogId::new(1));
    assert!(text.len() <= 300 + '…'.len_utf8() && text.ends_with('…'));
}

fn job(state: JobState) -> Job {
    Job {
        id: JobId::new(4),
        kind: JobKind::Adopt,
        target: "root@192.0.2.2:22".into(),
        node: Some(N2),
        state,
        revision: None,
        nodes: Vec::new(),
        steps: Vec::new(),
        error: None,
        created_by: None,
        started_at: at(0),
        finished_at: Some(at(30)),
    }
}

#[test]
fn an_adoption_is_an_event_once_and_a_failed_job_too() {
    let mut d = deriver();
    assert_eq!(d.job(&job(JobState::Running), at(10)), None);
    let adopted = d.job(&job(JobState::Succeeded), at(31)).unwrap();
    assert_eq!(
        (adopted.what.clone(), adopted.node, adopted.at),
        (EventKind::NodeAdopted, Some(N2), at(30))
    );
    assert_eq!(d.job(&job(JobState::Succeeded), at(32)), None);
    let mut failed = job(JobState::Failed);
    failed.id = JobId::new(5);
    assert_eq!(d.job(&failed, at(40)).unwrap().what.name(), "job_failed");
}

#[test]
fn roams_name_both_access_points() {
    let roam = Roam {
        mac: PHONE.parse::<MacAddr>().unwrap(),
        from: Attachment {
            node: N1,
            iface: "phy1-ap0".into(),
        },
        to: Attachment {
            node: N2,
            iface: "phy1-ap0".into(),
        },
        auth_alg: Some("ft".into()),
    };
    let event = deriver().roam(&roam, at(5));
    assert_eq!(event.node, Some(N2));
    assert!(matches!(
        event.what,
        EventKind::ClientRoamed { fast: true, .. }
    ));
}

#[test]
fn conditions_raised_before_a_restart_resume() {
    let link = "node:1~node:2~mesh";
    let seed = Seed {
        started: at(1_000),
        raised: vec![
            ("node:1/offline".into(), at(500)),
            (format!("node:2/mesh:{link}"), at(600)),
        ],
        clients: vec![client(PHONE, false, 1, 5)],
        facts: Vec::new(),
    };
    let mut d = Deriver::new(DeriveConfig::default(), seed);
    let back = set(&mut d, N1, NodeState::Online, 1_000);
    assert_eq!(
        back[0].what,
        EventKind::NodeOnline {
            down_ms: Some(500_000)
        }
    );
    set(&mut d, N2, NodeState::Online, 1_000);
    // The link is still gone: it stays down, raised once.
    assert_eq!(d.topology(&topology(&[]), at(1_001)), Vec::new());
    assert_eq!(d.tick(at(1_100)), Vec::new());
    let mesh = topology(&[edge("node:1", "node:2", "mesh", true, Some(80))]);
    assert_eq!(names(&d.topology(&mesh, at(1_200))), ["mesh_link_up"]);
    // A known client coming back joined; it is not new.
    let joined = d.clients(&[client(PHONE, true, 1, 1_300)], at(1_300));
    assert_eq!(names(&joined), ["client_joined"]);
}
