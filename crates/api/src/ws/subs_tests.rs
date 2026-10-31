//! [`Subscriptions`] over a scripted fleet: snapshots, how deltas merge between flushes, the
//! caps, topics replaced, and resyncs.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, PoisonError},
};

use serde_json::Value;
use tokio::sync::broadcast;
use uniwrt_core::{NodeId, SystemClock, Timestamp};
use uniwrt_engine::{Event, Fleet, Interest, NodeView};
use uniwrt_model::{
    event::{Attachment, NodeEvent, Roam, StationChange, StationEvent},
    facts::NodeFacts,
    log::{LogEntry, Severity},
    node::{NewNode, NodeRole},
    status::{NodeState, NodeStatus},
};
use uniwrt_node::proto::ops::Set;
use uniwrt_store::{NodeRepo as _, Store};
use uniwrt_topo::{Member, Topology};

use super::{message::ServerMessage, subs::Subscriptions};
use crate::{Api, testing};

#[derive(Default)]
struct FakeFleet {
    views: Mutex<BTreeMap<NodeId, NodeView>>,
    topology: Mutex<Arc<Topology>>,
}

impl FakeFleet {
    fn set(&self, node: NodeId, view: NodeView) {
        self.views().insert(node, view);
    }

    fn views(&self) -> std::sync::MutexGuard<'_, BTreeMap<NodeId, NodeView>> {
        self.views.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Fleet for FakeFleet {
    fn all(&self) -> Vec<(NodeId, NodeView)> {
        self.views()
            .iter()
            .map(|(&id, view)| (id, view.clone()))
            .collect()
    }

    fn node(&self, node: NodeId) -> Option<NodeView> {
        self.views().get(&node).cloned()
    }

    fn focus(&self, _: NodeId) -> Option<Interest> {
        None
    }

    fn follow_logs(&self, _: NodeId) -> Option<Interest> {
        None
    }

    fn subscribe(&self) -> broadcast::Receiver<Event> {
        broadcast::channel(1).1
    }

    fn topology(&self) -> Arc<Topology> {
        Arc::clone(&self.topology.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

struct Harness {
    _dir: tempfile::TempDir,
    store: Store,
    fleet: Arc<FakeFleet>,
    api: Api,
    subs: Subscriptions,
}

async fn harness() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("uniwrt.db"), Arc::new(SystemClock))
        .await
        .unwrap();
    let fleet = Arc::new(FakeFleet::default());
    let api = testing::api(store.clone(), fleet.clone(), Arc::new(SystemClock)).await;
    Harness {
        _dir: dir,
        store,
        fleet,
        api,
        subs: Subscriptions::default(),
    }
}

impl Harness {
    async fn adopt(&self, last_octet: u8) -> NodeId {
        let node = self
            .store
            .create_node(NewNode {
                name: format!("ap{last_octet}"),
                mac: format!("02:00:00:00:00:{last_octet:02x}").parse().unwrap(),
                board_name: "glinet,gl-mt3000".into(),
                host: format!("192.168.1.{last_octet}"),
                port: 22,
                role: NodeRole::Ap,
            })
            .await
            .unwrap();
        self.fleet.set(node.id, view(NodeState::Online, 0));
        node.id
    }

    async fn set(&mut self, topics: &[&str]) -> Vec<Value> {
        let topics = topics.iter().map(ToString::to_string).collect();
        values(&self.subs.set(&self.api, topics).await)
    }

    async fn apply(&mut self, event: Event) {
        self.subs.apply(&self.api, &event).await;
    }

    fn flush(&mut self) -> Vec<Value> {
        values(&self.subs.flush(&self.api))
    }
}

fn values(messages: &[ServerMessage]) -> Vec<Value> {
    messages
        .iter()
        .map(|message| serde_json::to_value(message).unwrap())
        .collect()
}

fn view(state: NodeState, since: i64) -> NodeView {
    let facts: NodeFacts =
        serde_json::from_str(include_str!("../../../model/fixtures/facts/gateway.json")).unwrap();
    NodeView {
        status: status(state, since),
        facts: Some(Arc::new(facts)),
    }
}

fn status(state: NodeState, since: i64) -> NodeStatus {
    NodeStatus {
        state,
        since: Timestamp::from_unix_ms(since),
        reasons: Vec::new(),
    }
}

fn station_event(node: NodeId, n: i64) -> Event {
    Event::Node {
        node,
        at: Timestamp::from_unix_ms(n),
        reported_at: None,
        event: NodeEvent::Station(StationEvent {
            mac: "02:00:00:00:04:00".parse().unwrap(),
            iface: "phy1-ap0".into(),
            change: StationChange::Authorized,
            signal_dbm: None,
            freq_mhz: None,
            auth_alg: None,
        }),
    }
}

fn roam(from: NodeId, to: NodeId) -> Event {
    let at = |node| Attachment {
        node,
        iface: "phy1-ap0".into(),
    };
    Event::Roam {
        at: Timestamp::from_unix_ms(1),
        roam: Roam {
            mac: "02:00:00:00:04:00".parse().unwrap(),
            from: at(from),
            to: at(to),
            auth_alg: Some("ft".into()),
        },
    }
}

fn log(severity: Severity) -> LogEntry {
    LogEntry {
        seq: None,
        time: Timestamp::from_unix_ms(1),
        facility: 3,
        severity,
        program: Some("hostapd".into()),
        pid: None,
        message: "hello".into(),
    }
}

fn keys(value: &Value) -> Vec<&str> {
    let mut keys: Vec<_> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    keys
}

#[tokio::test]
async fn a_node_topic_merges_status_and_changed_parts_until_the_flush() {
    let mut h = harness().await;
    let node = h.adopt(2).await;
    let topic = format!("node:{node}");
    let snapshot = h.set(&[&topic]).await;
    assert_eq!(snapshot.len(), 1);
    assert_eq!(snapshot[0]["type"], "node_snapshot");
    assert_eq!(snapshot[0]["topic"], topic.as_str());
    assert_eq!(snapshot[0]["node"]["node"]["id"], node.get());
    assert_eq!(snapshot[0]["node"]["status"]["state"], "online");
    assert!(snapshot[0]["node"]["facts"]["stations"].is_array());
    assert_eq!(h.flush(), Vec::<Value>::new());

    for since in 1..=3 {
        h.fleet.set(node, view(NodeState::Degraded, since));
        h.apply(Event::Status {
            node,
            status: status(NodeState::Degraded, since),
        })
        .await;
    }
    for set in [Set::System, Set::Stations, Set::System] {
        h.apply(Event::Facts {
            node,
            facts: h.fleet.node(node).unwrap().facts.unwrap(),
            changed: vec![set],
        })
        .await;
    }
    // Another node's news is not this topic's.
    h.apply(Event::Status {
        node: NodeId::new(99),
        status: status(NodeState::Offline, 0),
    })
    .await;
    let delta = h.flush();
    assert_eq!(delta.len(), 1);
    let delta = &delta[0];
    assert_eq!(delta["type"], "node_delta");
    assert_eq!(delta["status"]["since"], 3);
    assert_eq!(
        keys(&delta["facts"]),
        ["collected_at", "stations", "system"]
    );
    assert_eq!(delta["events"], Value::Array(Vec::new()));
    assert_eq!(delta["removed"], false);
    assert_eq!(h.flush(), Vec::<Value>::new());

    h.apply(Event::Removed { node }).await;
    let delta = h.flush();
    assert_eq!(delta[0]["removed"], true);
    assert_eq!(
        keys(&delta[0]),
        ["events", "removed", "roams", "topic", "type"]
    );
}

#[tokio::test]
async fn events_and_roams_queue_in_order_up_to_a_cap() {
    let mut h = harness().await;
    let node = h.adopt(2).await;
    let other = h.adopt(3).await;
    h.set(&[&format!("node:{node}")]).await;
    for n in 0..300 {
        h.apply(station_event(node, n)).await;
    }
    h.apply(station_event(other, 0)).await;
    h.apply(roam(other, node)).await;
    h.apply(roam(other, NodeId::new(99))).await;
    let delta = h.flush();
    let events = delta[0]["events"].as_array().unwrap();
    assert_eq!(events.len(), 256);
    assert_eq!(events[0]["at"], 44);
    assert_eq!(events[255]["at"], 299);
    assert_eq!(events[0]["event"]["kind"], "station");
    let roams = delta[0]["roams"].as_array().unwrap();
    assert_eq!(roams.len(), 1);
    assert_eq!(roams[0]["roam"]["to"]["node"], node.get());
    assert!(delta[0].get("status").is_none() && delta[0].get("facts").is_none());
}

#[tokio::test]
async fn logs_are_filtered_by_severity_and_overflow_is_counted() {
    let mut h = harness().await;
    let node = h.adopt(2).await;
    let topic = format!("logs?node={node}&min=warning");
    let snapshot = h.set(&[&topic]).await;
    assert_eq!(snapshot[0]["type"], "log_snapshot");
    assert_eq!(snapshot[0]["entries"], Value::Array(Vec::new()));
    let entries = [Severity::Info, Severity::Warning, Severity::Error];
    h.apply(Event::Log {
        node,
        entries: entries.map(log).into(),
        dropped: 2,
    })
    .await;
    let delta = h.flush();
    assert_eq!(delta[0]["type"], "log_delta");
    let severities: Vec<_> = delta[0]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["severity"].as_str().unwrap())
        .collect();
    assert_eq!(severities, ["warning", "error"]);
    assert_eq!(delta[0]["dropped"], 2);

    let many: Vec<_> = (0..1_500).map(|_| log(Severity::Error)).collect();
    h.apply(Event::Log {
        node,
        entries: many.into(),
        dropped: 0,
    })
    .await;
    let delta = h.flush();
    assert_eq!(delta[0]["entries"].as_array().unwrap().len(), 1_000);
    assert_eq!(delta[0]["dropped"], 500);
    assert_eq!(h.flush(), Vec::<Value>::new());
}

#[tokio::test]
async fn the_fleet_sends_changed_summaries_new_nodes_and_removals() {
    let mut h = harness().await;
    let first = h.adopt(2).await;
    let snapshot = h.set(&["fleet"]).await;
    assert_eq!(snapshot[0]["type"], "fleet_snapshot");
    assert_eq!(snapshot[0]["nodes"].as_array().unwrap().len(), 1);
    assert_eq!(snapshot[0]["nodes"][0]["clients"], 3);
    assert_eq!(snapshot[0]["nodes"][0]["model"], "GL.iNet GL-MT6000");

    // A status event that changes nothing in the summary sends nothing.
    h.apply(Event::Status {
        node: first,
        status: status(NodeState::Online, 0),
    })
    .await;
    assert_eq!(h.flush(), Vec::<Value>::new());

    h.fleet.set(first, view(NodeState::Offline, 5));
    h.apply(Event::Status {
        node: first,
        status: status(NodeState::Offline, 5),
    })
    .await;
    let second = h.adopt(3).await;
    h.apply(Event::Status {
        node: second,
        status: status(NodeState::Online, 0),
    })
    .await;
    let delta = h.flush();
    assert_eq!(delta[0]["type"], "fleet_delta");
    let nodes = delta[0]["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0]["status"]["state"], "offline");
    assert_eq!(nodes[1]["id"], second.get());

    // Unwatched but still adopted: shown as stopped. Deleted: removed.
    h.fleet.views().remove(&first);
    h.apply(Event::Removed { node: first }).await;
    let delta = h.flush();
    assert_eq!(
        delta[0]["nodes"][0]["status"]["reasons"][0]["kind"],
        "stopped"
    );
    assert!(h.store.delete_node(second).await.unwrap());
    h.apply(Event::Removed { node: second }).await;
    let delta = h.flush();
    assert_eq!(delta[0]["nodes"], Value::Array(Vec::new()));
    assert_eq!(delta[0]["removed"], serde_json::json!([second.get()]));
}

#[tokio::test]
async fn topics_are_replaced_and_bad_ones_refused() {
    let mut h = harness().await;
    let node = h.adopt(2).await;
    let topic = format!("node:{node}");
    let types = |messages: &[Value]| -> Vec<String> {
        messages
            .iter()
            .map(|message| message["type"].as_str().unwrap().to_owned())
            .collect()
    };
    let out = h.set(&["fleet", &topic, "fleet"]).await;
    assert_eq!(types(&out), ["fleet_snapshot", "node_snapshot"]);
    // Topics already held send no snapshot again.
    assert_eq!(h.set(&["fleet", &topic]).await, Vec::<Value>::new());
    let out = h.set(&[&topic, "weather", "node:42"]).await;
    assert_eq!(types(&out), ["error", "error"]);
    assert_eq!(out[0]["topic"], "node:42");
    assert_eq!(out[0]["message"], "There is no node 42");
    assert_eq!(out[1]["topic"], "weather");
    // The fleet went: its news is no longer gathered.
    h.apply(Event::Status {
        node,
        status: status(NodeState::Offline, 9),
    })
    .await;
    assert_eq!(types(&h.flush()), ["node_delta"]);

    let many: Vec<String> = (1..=33).map(|id| format!("node:{id}")).collect();
    let many: Vec<&str> = many.iter().map(String::as_str).collect();
    let out = h.set(&many).await;
    assert_eq!(types(&out), ["error"]);
    assert_eq!(out[0]["topic"], Value::Null);
}

#[tokio::test]
async fn a_resync_snapshots_every_topic_and_drops_those_gone() {
    let mut h = harness().await;
    let kept = h.adopt(2).await;
    let gone = h.adopt(3).await;
    h.set(&["fleet", &format!("node:{kept}"), &format!("node:{gone}")])
        .await;
    h.apply(station_event(kept, 1)).await;
    assert!(h.store.delete_node(gone).await.unwrap());
    let out = values(&h.subs.resync(&h.api).await);
    let types: Vec<_> = out
        .iter()
        .map(|message| message["type"].as_str().unwrap())
        .collect();
    assert_eq!(types, ["fleet_snapshot", "node_snapshot", "error"]);
    assert_eq!(out[0]["nodes"].as_array().unwrap().len(), 1);
    assert_eq!(out[2]["topic"], format!("node:{gone}").as_str());
    // What was pending before the resync is in the snapshot.
    assert_eq!(h.flush(), Vec::<Value>::new());
}

/// The topology of one gateway with the canonical facts, as of `ms`.
fn topology(ms: i64, clients: bool) -> Arc<Topology> {
    let mut facts: NodeFacts =
        serde_json::from_str(include_str!("../../../model/fixtures/facts/gateway.json")).unwrap();
    facts.collected_at = Timestamp::from_unix_ms(ms);
    if !clients {
        facts.stations.clear();
        facts.neighbors.clear();
        facts.fdb.clear();
        facts.batman.clear();
    }
    let member = Member {
        id: NodeId::new(1),
        role: NodeRole::Gateway,
        mac: "02:00:00:00:00:01".parse().unwrap(),
        state: NodeState::Online,
        facts: Some(&facts),
    };
    Arc::new(uniwrt_topo::build(&[member], None))
}

#[tokio::test]
async fn the_topology_topic_sends_what_changed_against_what_the_client_has() {
    let mut h = harness().await;
    *h.fleet.topology.lock().unwrap() = topology(1_000, true);
    let snapshot = h.set(&["topology"]).await;
    assert_eq!(snapshot[0]["type"], "topology_snapshot");
    let clients = snapshot[0]["topology"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|node| node["kind"] == "client")
        .count();
    assert!(clients > 0);
    assert_eq!(h.flush(), Vec::<Value>::new());
    // Two builds between flushes: one delta, against the snapshot.
    h.apply(Event::Topology {
        topology: topology(2_000, true),
    })
    .await;
    h.apply(Event::Topology {
        topology: topology(3_000, false),
    })
    .await;
    let delta = h.flush();
    assert_eq!(delta.len(), 1);
    assert_eq!(delta[0]["type"], "topology_delta");
    let removed = delta[0]["delta"]["removed_nodes"].as_array().unwrap();
    let gone = removed
        .iter()
        .filter(|id| id.as_str().unwrap().starts_with("client:"));
    assert_eq!(gone.count(), clients);
    assert_eq!(h.flush(), Vec::<Value>::new());
    // A build that changes nothing sends nothing.
    h.apply(Event::Topology {
        topology: topology(3_000, false),
    })
    .await;
    assert_eq!(h.flush(), Vec::<Value>::new());
}
