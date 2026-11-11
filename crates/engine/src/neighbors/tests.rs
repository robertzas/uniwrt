//! The neighbor reports over a store and an engine watching two nodes (the lab gateway's
//! capture), the `rrm` op answered by hand: each managed node's BSS gets the other's as its
//! neighbor, a node in observe mode is left alone, and a refusal raises a condition that clears
//! once the node takes its lists.

use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
    time::Duration,
};

use serde_json::{Value, json};
use tokio::sync::broadcast;
use uniwrt_core::{MacAddr, ManualClock, NodeId, Timestamp};
use uniwrt_model::{
    events::{EventKind, NewEvent},
    node::{NewNode, NodeRole},
    site::{
        SiteConfig,
        node::{ManagedPackage, NodeConfig},
        revision::{NewRevision, RevisionKind},
    },
};
use uniwrt_node::Error;
use uniwrt_store::{NodeRepo as _, RevisionRepo as _, Store};

use super::{NeighborReports, NeighborsConfig};
use crate::{
    Engine, EngineConfig, Event, Identity,
    testing::{self, Call, Script, hello, running},
};

const NOW: Timestamp = Timestamp::from_unix_secs(1_800_000_000);
const QUIET: Duration = Duration::from_millis(300);
const WAIT: Duration = Duration::from_secs(5);

struct Harness {
    _dir: tempfile::TempDir,
    clock: Arc<ManualClock>,
    engine: Engine<testing::SharedLink>,
    bus: broadcast::Receiver<Event>,
    reports: Option<NeighborReports>,
    nodes: Vec<NodeId>,
    scripts: Vec<Script>,
    /// What the `rrm` op fails with on the first node, if it does.
    refuse: Arc<Mutex<Option<String>>>,
}

/// Node `k`'s one BSS with 802.11k, on `phy1-ap0`.
fn own(k: u8) -> Value {
    json!({"ifname": "phy1-ap0", "bssid": format!("02:00:00:00:0{k}:01"), "ssid": "home", "nr": format!("0{k}")})
}

impl Harness {
    /// Two nodes; the second manages `wireless` only if `both`.
    async fn start(both: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(ManualClock::new(NOW));
        let store = Store::open(dir.path().join("test.db"), clock.clone())
            .await
            .unwrap();
        let mut config = SiteConfig::defaults(None);
        let mut nodes = Vec::new();
        for k in 1..=2u8 {
            let node = NewNode {
                name: format!("ap{k}"),
                mac: MacAddr::new([2, 0, 0, 0, 0, k]),
                board_name: "x".to_owned(),
                host: format!("10.0.0.{k}"),
                port: 22,
                role: NodeRole::Ap,
            };
            let node = store.create_node(node).await.unwrap().id;
            let mut node_config = NodeConfig::new(node);
            if k == 1 || both {
                node_config.managed = BTreeSet::from([ManagedPackage::Wireless]);
            }
            config.nodes.push(node_config);
            nodes.push(node);
        }
        let revision = NewRevision {
            base: None,
            kind: RevisionKind::Defaults,
            restored_from: None,
            author_id: None,
            author: None,
            message: "test".to_owned(),
            config,
        };
        store.commit_revision(revision, Vec::new()).await.unwrap();
        let engine = Engine::new(EngineConfig::default(), clock.clone());
        let bus = engine.subscribe();
        let config = NeighborsConfig {
            tick_every: Duration::from_millis(50),
            ..NeighborsConfig::default()
        };
        let reports = NeighborReports::start(&engine, store, None, config);
        let refuse = Arc::new(Mutex::new(None::<String>));
        let mut scripts = Vec::new();
        for (k, &node) in (1..=2u8).zip(&nodes) {
            let (link, script) = testing::link();
            let refusal = (k == 1).then(|| Arc::clone(&refuse));
            script.answer(move |op, args| match op {
                "collect" => {
                    let sets: Vec<uniwrt_node::proto::ops::Set> =
                        serde_json::from_value(args["sets"].clone()).unwrap();
                    Some(Ok(testing::collected(&sets)))
                }
                "rrm" => Some(
                    match refusal.as_ref().and_then(|r| r.lock().unwrap().clone()) {
                        Some(msg) => Err(Error::Remote {
                            op: "rrm".to_owned(),
                            code: "failed".to_owned(),
                            msg,
                        }),
                        None => Ok(json!({"own": [own(k)], "set": [], "failed": []})),
                    },
                ),
                _ => None,
            });
            let identity = Identity {
                role: NodeRole::Ap,
                mac: MacAddr::new([2, 0, 0, 0, 0, k]),
            };
            engine.add(node, identity, link, None).await;
            script.set_state(running(hello(true)));
            scripts.push(script);
        }
        Self {
            _dir: dir,
            clock,
            engine,
            bus,
            reports: Some(reports),
            nodes,
            scripts,
            refuse,
        }
    }

    /// The next `rrm` request to node `i` within `wait`, if any.
    async fn rrm(&mut self, i: usize, wait: Duration) -> Option<Call> {
        tokio::time::timeout(wait, self.scripts[i].call_of("rrm"))
            .await
            .ok()
    }

    /// Node `i`'s requests until one carries `lists`.
    async fn lists(&mut self, i: usize, lists: &Value) {
        let deadline = tokio::time::Instant::now() + WAIT;
        loop {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            let call = self.rrm(i, left).await;
            let call = call.unwrap_or_else(|| panic!("node {i} never got {lists}"));
            if call.args["lists"] == *lists {
                return;
            }
        }
    }

    /// The next event noted of `kind`.
    async fn noted(&mut self, kind: fn(&EventKind) -> bool) -> NewEvent {
        loop {
            match tokio::time::timeout(Duration::from_secs(10), self.bus.recv()).await {
                Ok(Ok(Event::Noted { event })) if kind(&event.what) => return event,
                Ok(Ok(_) | Err(broadcast::error::RecvError::Lagged(_))) => {}
                other => panic!("no event noted: {other:?}"),
            }
        }
    }

    async fn stop(mut self) {
        if let Some(reports) = self.reports.take() {
            reports.stop().await;
        }
        self.engine.shutdown().await;
    }
}

#[tokio::test]
async fn each_managed_bss_gets_the_others_of_its_ssid() {
    let mut h = Harness::start(true).await;
    let entry = |k: u8| json!([format!("02:00:00:00:0{k}:01"), "home", format!("0{k}")]);
    h.lists(0, &json!({"phy1-ap0": [entry(2)]})).await;
    h.lists(1, &json!({"phy1-ap0": [entry(1)]})).await;
    h.stop().await;
}

#[tokio::test]
async fn observe_mode_leaves_the_node_alone() {
    let mut h = Harness::start(false).await;
    // The managed node learns nothing of the other: its BSS has no neighbors.
    h.lists(0, &json!({"phy1-ap0": []})).await;
    assert!(h.rrm(1, QUIET).await.is_none());
    h.stop().await;
}

#[tokio::test]
async fn a_refusal_raises_a_condition_that_clears_when_the_node_takes_them() {
    let mut h = Harness::start(true).await;
    *h.refuse.lock().unwrap() = Some("Connection failed".to_owned());
    let failed = h
        .noted(|k| matches!(k, EventKind::NeighborReportsFailed { .. }))
        .await;
    assert_eq!(
        failed.what,
        EventKind::NeighborReportsFailed {
            error: "Connection failed".to_owned()
        }
    );
    let condition = failed.condition.unwrap();
    assert_eq!(
        condition.key,
        format!("node:{}/neighbor-reports", h.nodes[0])
    );
    assert!(condition.raised);
    // It waits `retry` before asking again.
    h.scripts[0].drain();
    assert!(h.rrm(0, QUIET).await.is_none());

    *h.refuse.lock().unwrap() = None;
    h.clock.advance(Duration::from_mins(2));
    let ok = h.noted(|k| matches!(k, EventKind::NeighborReportsOk)).await;
    assert!(!ok.condition.unwrap().raised);
    h.stop().await;
}
