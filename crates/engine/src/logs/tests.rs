//! The recorder over a store, an engine with one node (its link never connects) and syslog
//! fed by hand: the agent follows the log, resumes after what is stored, gives way to syslog
//! and takes over again when syslog goes quiet or closes; other hosts are stored by name.

use std::{net::IpAddr, sync::Arc, time::Duration};

use tokio::sync::{broadcast, mpsc};
use uniwrt_core::{ManualClock, NodeId, Timestamp};
use uniwrt_model::{
    facts::NodeFacts,
    log::{LogEntry, LogFilter, LogRecord, LogVia, NewLog, Severity},
};
use uniwrt_store::{LogRepo as _, Store};
use uniwrt_syslog::{Message, Received};

use super::{LogRecorder, LogsConfig};
use crate::{
    Engine, EngineConfig, Event,
    testing::{SharedLink, identity, link},
};

const NODE: NodeId = NodeId::new(1);
const START: Timestamp = Timestamp::from_unix_secs(1_800_000_000);
/// The gateway fixture's LAN address.
const GATEWAY: [u8; 4] = [10, 0, 0, 1];

fn entry(message: &str) -> LogEntry {
    LogEntry {
        seq: None,
        time: START,
        facility: 3,
        severity: Severity::Notice,
        program: Some("netifd".into()),
        pid: None,
        message: message.into(),
    }
}

fn syslog(peer: [u8; 4], hostname: Option<&str>, tcp: bool, text: &str) -> Received {
    Received::Message {
        peer: IpAddr::from(peer),
        at: START,
        tcp,
        message: Message {
            hostname: hostname.map(str::to_owned),
            timed: false,
            entry: entry(text),
        },
    }
}

struct Harness {
    _dir: tempfile::TempDir,
    store: Store,
    engine: Engine<SharedLink>,
    syslog: mpsc::Sender<Received>,
    bus: broadcast::Receiver<Event>,
    recorder: LogRecorder,
}

impl Harness {
    async fn start(stored: &[&str]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(ManualClock::new(START));
        let store = Store::open(dir.path().join("test.db"), clock.clone())
            .await
            .unwrap();
        let earlier: Vec<_> = stored
            .iter()
            .map(|text| NewLog {
                received: START,
                node: Some(NODE),
                host: None,
                via: LogVia::Agent,
                entry: entry(text),
                pattern: None,
            })
            .collect();
        store.record_logs(earlier).await.unwrap();
        let engine = Engine::new(EngineConfig::default(), clock);
        let facts: NodeFacts =
            serde_json::from_str(include_str!("../../../model/fixtures/facts/gateway.json"))
                .unwrap();
        let (shared, _script) = link();
        engine.add(NODE, identity(), shared, Some(facts)).await;
        let bus = engine.subscribe();
        let (syslog, rx) = mpsc::channel(16);
        let config = LogsConfig {
            flush_every: Duration::from_millis(5),
            syslog_quiet: Duration::from_millis(200),
            ..LogsConfig::default()
        };
        let recorder = LogRecorder::start(&engine, store.clone(), Some(rx), config).await;
        Self {
            _dir: dir,
            store,
            engine,
            syslog,
            bus,
            recorder,
        }
    }

    fn agent(&self, texts: &[&str]) {
        self.engine.context().publish(Event::Log {
            node: NODE,
            entries: texts
                .iter()
                .map(|text| entry(text))
                .collect::<Vec<_>>()
                .into(),
            dropped: 0,
        });
    }

    /// The next entries announced as stored.
    async fn logged(&mut self) -> Vec<LogRecord> {
        let wait = async {
            loop {
                if let Ok(Event::Logged { records }) = self.bus.recv().await {
                    return records.to_vec();
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(5), wait)
            .await
            .unwrap()
    }

    async fn agent_follows(&self, follows: bool) {
        let wait = async {
            while (self.engine.interests(NODE).unwrap().logs > 0) != follows {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        };
        tokio::time::timeout(Duration::from_secs(5), wait)
            .await
            .unwrap();
    }

    async fn texts(&self) -> Vec<String> {
        let mut records = self
            .store
            .logs(LogFilter::default(), None, 100)
            .await
            .unwrap();
        records.reverse();
        records.into_iter().map(|r| r.entry.message).collect()
    }
}

#[tokio::test]
async fn the_agent_resumes_after_what_is_stored_and_entries_are_annotated() {
    let mut h = Harness::start(&["a", "b"]).await;
    h.agent_follows(true).await;
    // Its backfill repeats a and b.
    h.agent(&["a", "b", "Interface 'wan' is now down"]);
    let logged = h.logged().await;
    assert_eq!(logged.len(), 1);
    assert_eq!(logged[0].node, Some(NODE));
    assert_eq!((logged[0].via, logged[0].received), (LogVia::Agent, START));
    assert_eq!(logged[0].pattern.as_deref(), Some("interface-down"));
    // Later batches are stored whole, a repeated line too.
    h.agent(&["b", "c"]);
    assert_eq!(h.logged().await.len(), 2);
    assert_eq!(
        h.texts().await,
        ["a", "b", "Interface 'wan' is now down", "b", "c"]
    );
    h.recorder.stop().await;
}

#[tokio::test]
async fn syslog_takes_over_from_the_agent_and_hands_back() {
    let mut h = Harness::start(&[]).await;
    h.agent_follows(true).await;
    h.agent(&["a"]);
    h.logged().await;
    // Syslog from the node's address: the agent stops; a copy of a is skipped.
    h.syslog
        .send(syslog(GATEWAY, None, false, "a"))
        .await
        .unwrap();
    h.syslog
        .send(syslog(GATEWAY, None, false, "s1"))
        .await
        .unwrap();
    let logged = h.logged().await;
    assert_eq!(logged.len(), 1);
    assert_eq!(
        (logged[0].node, logged[0].via),
        (Some(NODE), LogVia::Syslog)
    );
    h.agent_follows(false).await;
    // Quiet over UDP: the agent follows again, its backfill skipped through s1.
    h.agent_follows(true).await;
    h.agent(&["a", "s1", "new"]);
    let logged = h.logged().await;
    assert_eq!(
        logged
            .iter()
            .map(|r| r.entry.message.as_str())
            .collect::<Vec<_>>(),
        ["new"]
    );
    // Over TCP (the node found by its host name), until the connection closes.
    h.syslog
        .send(syslog([192, 0, 2, 1], Some("Gateway"), true, "t1"))
        .await
        .unwrap();
    h.logged().await;
    h.agent_follows(false).await;
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(h.engine.interests(NODE).unwrap().logs, 0);
    h.syslog
        .send(Received::Closed {
            peer: IpAddr::from(GATEWAY),
        })
        .await
        .unwrap();
    h.agent_follows(true).await;
    assert_eq!(h.texts().await, ["a", "s1", "new", "t1"]);
    h.recorder.stop().await;
}

#[tokio::test]
async fn other_hosts_are_stored_by_name_or_address() {
    let mut h = Harness::start(&[]).await;
    h.syslog
        .send(syslog([192, 0, 2, 9], Some("switch"), false, "x"))
        .await
        .unwrap();
    h.syslog
        .send(syslog([192, 0, 2, 9], None, false, "y"))
        .await
        .unwrap();
    let mut logged = h.logged().await;
    if logged.len() < 2 {
        logged.extend(h.logged().await);
    }
    let hosts: Vec<_> = logged.iter().map(|r| (r.node, r.host.as_deref())).collect();
    assert_eq!(hosts, [(None, Some("switch")), (None, Some("192.0.2.9"))]);
    // The node's agent keeps following.
    assert_eq!(h.engine.interests(NODE).unwrap().logs, 1);
    h.recorder.stop().await;
}

#[test]
fn an_address_two_nodes_claim_names_neither() {
    let mut map = std::collections::HashMap::new();
    super::claim(&mut map, "openwrt", NODE);
    super::claim(&mut map, "openwrt", NODE);
    assert_eq!(map["openwrt"], Some(NODE));
    super::claim(&mut map, "openwrt", NodeId::new(2));
    super::claim(&mut map, "openwrt", NODE);
    assert_eq!(map["openwrt"], None);
}
