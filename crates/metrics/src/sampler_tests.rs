//! The windows over the canonical gateway's facts, hand-made topologies and client samples.

use uniwrt_clients::ClientSample;
use uniwrt_core::{NodeId, Timestamp};
use uniwrt_model::{
    facts::NodeFacts,
    metric::{Entity, Metric, Sample},
    status::NodeState,
};
use uniwrt_topo::{Medium, TopoEdge, TopoKind, TopoNode, Topology, Traffic};

use super::Sampler;

const NODE: NodeId = NodeId::new(1);

fn gateway(at: Timestamp) -> NodeFacts {
    let mut facts: NodeFacts =
        serde_json::from_str(include_str!("../../model/fixtures/facts/gateway.json")).unwrap();
    facts.collected_at = at;
    facts
}

fn secs(secs: i64) -> Timestamp {
    Timestamp::from_unix_secs(1_800_000_000 + secs)
}

fn value(samples: &[Sample], entity: &Entity, metric: Metric) -> Option<f64> {
    samples
        .iter()
        .find(|sample| &sample.entity == entity && sample.metric == metric)
        .map(|sample| sample.value)
}

fn device(name: &str) -> Entity {
    Entity::Device {
        node: NODE,
        name: name.to_owned(),
    }
}

#[test]
fn gauges_come_from_every_announcement_and_average_over_the_window() {
    let mut windows = Sampler::new();
    let mut facts = gateway(secs(0));
    windows.facts(NODE, &facts, false);
    facts.collected_at = secs(10);
    facts.system.cpu_percent = Some(10.5);
    windows.facts(NODE, &facts, false);
    // The window [0, 15) closes once its grace passed.
    assert_eq!(windows.drain(secs(19)), []);
    let samples = windows.drain(secs(20));
    let node = Entity::Node(NODE);
    assert_eq!(value(&samples, &node, Metric::Cpu), Some(7.0));
    assert_eq!(value(&samples, &node, Metric::Temperature), Some(51.5));
    let memory = value(&samples, &node, Metric::Memory).unwrap();
    assert!((memory - 21.287).abs() < 0.01, "{memory}");
    assert!((value(&samples, &node, Metric::Load).unwrap() - 0.12).abs() < 1e-6);
    // Three Wi-Fi clients at the APs; the mesh peer is not one.
    assert_eq!(value(&samples, &node, Metric::Stations), Some(3.0));
    let radio1 = Entity::Radio {
        node: NODE,
        name: "radio1".to_owned(),
    };
    assert_eq!(value(&samples, &radio1, Metric::Stations), Some(2.0));
    assert_eq!(value(&samples, &radio1, Metric::Busy), Some(8.25));
    assert_eq!(value(&samples, &radio1, Metric::Noise), Some(-95.0));
    assert!(samples.iter().all(|sample| sample.at == secs(0)));
    // Nothing is left, and what comes late for a drained window is dropped.
    facts.collected_at = secs(14);
    windows.facts(NODE, &facts, false);
    assert_eq!(windows.drain(secs(60)), []);
}

fn lan1(facts: &mut NodeFacts) -> &mut uniwrt_model::facts::DeviceFacts {
    facts
        .devices
        .iter_mut()
        .find(|device| device.name == "lan1")
        .unwrap()
}

#[test]
fn device_rates_come_from_counters_weighted_by_time() {
    let mut windows = Sampler::new();
    let mut facts = gateway(secs(0));
    windows.facts(NODE, &facts, true);
    let base = lan1(&mut facts).counters.rx_bytes;
    // 1 MB over 2 s, then nothing over 8 s: 1 MB × 8 bits / 10 s.
    facts.collected_at = secs(2);
    lan1(&mut facts).counters.rx_bytes = base + 1_000_000;
    windows.facts(NODE, &facts, true);
    facts.collected_at = secs(10);
    windows.facts(NODE, &facts, true);
    // A reading without new device counters gives no rate.
    facts.collected_at = secs(12);
    lan1(&mut facts).counters.rx_bytes = base + 9_000_000;
    windows.facts(NODE, &facts, false);
    let samples = windows.drain(secs(30));
    assert_eq!(
        value(&samples, &device("lan1"), Metric::RxBps),
        Some(800_000.0)
    );
    assert_eq!(value(&samples, &device("lan1"), Metric::TxBps), Some(0.0));
    assert_eq!(value(&samples, &device("lo"), Metric::RxBps), None);

    // A counter that went backwards (a reboot) gives no rate, and neither does a long gap.
    facts.collected_at = secs(20);
    lan1(&mut facts).counters.rx_bytes = 5;
    windows.facts(NODE, &facts, true);
    facts.collected_at = secs(500);
    lan1(&mut facts).counters.rx_bytes = 5_000_000;
    windows.facts(NODE, &facts, true);
    let samples = windows.drain(secs(600));
    assert_eq!(value(&samples, &device("lan1"), Metric::RxBps), None);
    let rates_at = |at| {
        samples
            .iter()
            .filter(|sample| sample.at == at && matches!(sample.entity, Entity::Device { .. }))
            .count()
    };
    assert!(rates_at(secs(15)) > 0);
    assert_eq!(rates_at(secs(495)), 0);

    // A forgotten node starts over.
    windows.forget(NODE);
    facts.collected_at = secs(700);
    windows.facts(NODE, &facts, true);
    let samples = windows.drain(secs(800));
    assert_ne!(samples, []);
    assert_eq!(value(&samples, &device("lan1"), Metric::RxBps), None);
}

fn vertex(id: &str, kind: TopoKind, node: Option<i64>, state: Option<NodeState>) -> TopoNode {
    TopoNode {
        id: id.to_owned(),
        kind,
        node: node.map(NodeId::new),
        mac: None,
        name: None,
        ips: Vec::new(),
        state,
        cpu_percent: None,
        memory_percent: None,
        parent: None,
        uplink: None,
        hops: None,
    }
}

fn edge(a: &str, b: &str, medium: Medium, up: bool) -> TopoEdge {
    TopoEdge {
        id: format!("{a}~{b}"),
        a: a.to_owned(),
        b: b.to_owned(),
        medium,
        a_port: None,
        b_port: None,
        up,
        band: None,
        ssid: None,
        signal_dbm: None,
        tq: None,
        capacity_mbps: None,
        quality: None,
        traffic: None,
    }
}

#[test]
fn builds_give_the_wan_clients_nodes_and_mesh_links() {
    let online = Some(NodeState::Online);
    let mut wan = edge("internet", "node:1", Medium::Wan, true);
    wan.traffic = Some(Traffic {
        at: secs(1),
        down_bytes: 0,
        up_bytes: 0,
        down_bps: Some(50_000_000),
        up_bps: Some(2_000_000),
    });
    let mut mesh = edge("node:1", "node:2", Medium::Mesh, true);
    mesh.signal_dbm = Some(-61);
    mesh.tq = Some(240);
    mesh.quality = Some(80);
    mesh.capacity_mbps = Some(400);
    let topology = Topology {
        at: secs(1),
        nodes: vec![
            vertex("internet", TopoKind::Internet, None, None),
            vertex("node:1", TopoKind::Gateway, Some(1), online),
            vertex("node:2", TopoKind::Ap, Some(2), online),
            vertex("node:3", TopoKind::Ap, Some(3), Some(NodeState::Offline)),
            vertex("client:a", TopoKind::Client, None, None),
            vertex("client:b", TopoKind::Client, None, None),
        ],
        edges: vec![
            wan,
            mesh,
            edge("node:2", "client:a", Medium::Wifi, true),
            edge("node:2", "client:b", Medium::Wifi, false),
        ],
    };
    let mut windows = Sampler::new();
    windows.topology(&topology);
    let samples = windows.drain(secs(30));
    assert_eq!(value(&samples, &Entity::Site, Metric::DownBps), Some(50e6));
    assert_eq!(value(&samples, &Entity::Site, Metric::UpBps), Some(2e6));
    assert_eq!(value(&samples, &Entity::Site, Metric::Clients), Some(1.0));
    assert_eq!(
        value(&samples, &Entity::Site, Metric::NodesOnline),
        Some(2.0)
    );
    let link = Entity::Link("node:1~node:2".to_owned());
    assert_eq!(value(&samples, &link, Metric::Signal), Some(-61.0));
    assert_eq!(value(&samples, &link, Metric::Tq), Some(240.0));
    assert_eq!(value(&samples, &link, Metric::Quality), Some(80.0));
    assert_eq!(value(&samples, &link, Metric::Capacity), Some(400.0));
}

#[test]
fn client_samples_become_series() {
    let mac = "f0:18:98:00:00:01".parse().unwrap();
    let sample = |at, signal| ClientSample {
        at,
        signal_dbm: Some(signal),
        snr_db: Some(signal + 95),
        tx_kbps: Some(866_700),
        rx_kbps: None,
        down_bps: Some(1_000),
        up_bps: Some(500),
        score: Some(90),
    };
    let mut windows = Sampler::new();
    windows.clients(&[(mac, sample(secs(0), -50)), (mac, sample(secs(5), -60))]);
    windows.clients(&[(mac, sample(secs(15), -70))]);
    let samples = windows.drain(secs(35));
    let client = Entity::Client(mac);
    let at = |at| {
        samples
            .iter()
            .find(|s| s.entity == client && s.metric == Metric::Signal && s.at == at)
            .map(|s| s.value)
    };
    assert_eq!(at(secs(0)), Some(-55.0));
    assert_eq!(at(secs(15)), Some(-70.0));
    assert_eq!(value(&samples, &client, Metric::TxRate), Some(866_700.0));
    assert_eq!(value(&samples, &client, Metric::RxRate), None);
    assert_eq!(value(&samples, &client, Metric::Score), Some(90.0));
}
