//! The bans over a store and an engine watching the lab gateway's capture, the `ban` op answered
//! by hand: a block bans the client on the managed node and an unblock lifts it, bans are
//! renewed before they lapse, a kick deauthenticates and answers where from, a node in observe
//! mode is left alone, and a refusal raises a condition that clears once the node takes them.

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
    site::{
        SiteConfig,
        node::{ManagedPackage, NodeConfig},
        revision::{NewRevision, RevisionKind},
    },
};
use uniwrt_node::Error;
use uniwrt_store::{NodeRepo as _, RevisionRepo as _, Store};

use super::{Bans, BansConfig, ClientBans, KickedFrom};
use crate::{
    Engine, EngineConfig, Event,
    testing::{self, Call, Script, hello, identity, running},
};

const NOW: Timestamp = Timestamp::from_unix_secs(1_800_000_000);
const QUIET: Duration = Duration::from_millis(300);
const WAIT: Duration = Duration::from_secs(5);

struct Harness {
    _dir: tempfile::TempDir,
    clock: Arc<ManualClock>,
    engine: Engine<testing::SharedLink>,
    bus: broadcast::Receiver<Event>,
    bans: Option<ClientBans>,
    node: NodeId,
    script: Script,
    /// What the `ban` op fails with, if it does.
    refuse: Arc<Mutex<Option<String>>>,
}

/// What the agent answers: every client named is kicked off `phy1-ap0` if a ban or kick names it.
fn banned(args: &Value) -> Value {
    let kicked: Vec<Value> = args["bans"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|b| b["ms"] != 0 || b["kick"] == true)
        .map(|b| json!({"ifname": "phy1-ap0", "addr": b["addr"]}))
        .collect();
    json!({"bsses": ["phy1-ap0", "phy2-ap0"], "kicked": kicked, "failed": []})
}

impl Harness {
    async fn start(managed: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(ManualClock::new(NOW));
        let store = Store::open(dir.path().join("test.db"), clock.clone())
            .await
            .unwrap();
        let node = store
            .create_node(uniwrt_model::node::NewNode {
                name: "gw".to_owned(),
                mac: identity().mac,
                board_name: "x".to_owned(),
                host: "10.0.0.1".to_owned(),
                port: 22,
                role: uniwrt_model::node::NodeRole::Gateway,
            })
            .await
            .unwrap()
            .id;
        let mut config = SiteConfig::defaults(None);
        let mut node_config = NodeConfig::new(node);
        if managed {
            node_config.managed = BTreeSet::from([ManagedPackage::Wireless]);
        }
        config.nodes = vec![node_config];
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
        let config = BansConfig {
            tick_every: Duration::from_millis(20),
            ..BansConfig::default()
        };
        let bans = ClientBans::start(&engine, store, None, config);
        let (link, script) = testing::link();
        let refuse = Arc::new(Mutex::new(None::<String>));
        let refusal = Arc::clone(&refuse);
        script.answer(move |op, args| match op {
            "collect" => {
                let sets: Vec<uniwrt_node::proto::ops::Set> =
                    serde_json::from_value(args["sets"].clone()).unwrap();
                Some(Ok(testing::collected(&sets)))
            }
            "ban" => Some(match refusal.lock().unwrap().clone() {
                Some(msg) => Err(Error::Remote {
                    op: "ban".to_owned(),
                    code: "failed".to_owned(),
                    msg,
                }),
                None => Ok(banned(args)),
            }),
            _ => None,
        });
        engine.add(node, identity(), link, None).await;
        Self {
            _dir: dir,
            clock,
            engine,
            bus,
            bans: Some(bans),
            node,
            script,
            refuse,
        }
    }

    fn handle(&self) -> Bans {
        self.bans.as_ref().unwrap().handle()
    }

    /// Brings the node up and answers a Wi-Fi client of it.
    async fn up(&self) -> MacAddr {
        self.script.set_state(running(hello(true)));
        let deadline = tokio::time::Instant::now() + WAIT;
        loop {
            let wifi = self.engine.clients().into_iter().find(|c| {
                c.online
                    && c.connection
                        .as_ref()
                        .is_some_and(|c| c.node == Some(self.node) && c.band.is_some())
            });
            if let Some(client) = wifi {
                return client.mac;
            }
            assert!(tokio::time::Instant::now() < deadline, "no Wi-Fi client");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// The next `ban` request within `wait`, if any.
    async fn ban(&mut self, wait: Duration) -> Option<Call> {
        tokio::time::timeout(wait, self.script.call_of("ban"))
            .await
            .ok()
    }

    /// The next event noted.
    async fn noted(&mut self) -> NewEvent {
        loop {
            match tokio::time::timeout(Duration::from_secs(10), self.bus.recv()).await {
                Ok(Ok(Event::Noted { event })) => return event,
                Ok(Ok(_) | Err(broadcast::error::RecvError::Lagged(_))) => {}
                other => panic!("no event noted: {other:?}"),
            }
        }
    }

    async fn stop(mut self) {
        if let Some(bans) = self.bans.take() {
            bans.stop().await;
        }
        self.engine.shutdown().await;
    }
}

/// A request's entries: (address, ms, kick).
fn entries(call: &Call) -> Vec<(String, u64, bool)> {
    call.args["bans"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| {
            (
                b["addr"].as_str().unwrap().to_owned(),
                b["ms"].as_u64().unwrap(),
                b["kick"].as_bool().unwrap(),
            )
        })
        .collect()
}

#[tokio::test]
async fn a_block_bans_the_client_until_unblocked_and_is_renewed() {
    let mut h = Harness::start(true).await;
    let mac = h.up().await;
    assert!(h.ban(QUIET).await.is_none(), "nothing wanted yet");

    let record = h
        .engine
        .block_client(mac, true, Some("admin".to_owned()))
        .unwrap();
    assert_eq!(record.blocked, Some(NOW));
    let call = h.ban(WAIT).await.unwrap();
    assert_eq!(entries(&call), [(mac.to_string(), 600_000, false)]);
    let event = h.noted().await;
    assert_eq!(
        event.what,
        EventKind::ClientBlocked {
            by: Some("admin".to_owned())
        }
    );
    assert_eq!(event.client, Some(mac));
    // Blocked again, it keeps its time and says nothing new.
    h.clock.advance(Duration::from_secs(1));
    let again = h.engine.block_client(mac, true, None).unwrap();
    assert_eq!(again.blocked, Some(NOW));
    // Held: nothing until it is about to lapse.
    assert!(h.ban(QUIET).await.is_none());
    h.clock.advance(Duration::from_mins(6));
    let renewed = h.ban(WAIT).await.unwrap();
    assert_eq!(entries(&renewed), [(mac.to_string(), 600_000, false)]);

    h.engine.block_client(mac, false, None).unwrap();
    let lift = h.ban(WAIT).await.unwrap();
    assert_eq!(entries(&lift), [(mac.to_string(), 0, false)]);
    assert!(h.ban(QUIET).await.is_none(), "lifted once");
    h.stop().await;
}

#[tokio::test]
async fn a_kick_deauthenticates_the_client_and_bans_it_for_a_while() {
    let mut h = Harness::start(true).await;
    let mac = h.up().await;
    let kicked = h
        .handle()
        .kick(mac, Duration::from_mins(1), Some("admin".to_owned()))
        .await
        .unwrap();
    assert_eq!(kicked.nodes, 1);
    assert_eq!(
        kicked.from,
        [KickedFrom {
            node: h.node,
            ifname: "phy1-ap0".to_owned()
        }]
    );
    assert_eq!(kicked.failed, []);
    let call = h.ban(WAIT).await.unwrap();
    assert_eq!(entries(&call), [(mac.to_string(), 60_000, true)]);
    let event = h.noted().await;
    assert_eq!(
        event.what,
        EventKind::ClientKicked {
            ban_secs: 60,
            by: Some("admin".to_owned())
        }
    );
    // The ban ends on its own: nothing to renew or lift.
    h.clock.advance(Duration::from_mins(2));
    assert!(h.ban(QUIET).await.is_none());

    // Without a ban: a kick alone.
    h.handle().kick(mac, Duration::ZERO, None).await.unwrap();
    let call = h.ban(WAIT).await.unwrap();
    assert_eq!(entries(&call), [(mac.to_string(), 0, true)]);
    assert!(
        h.handle()
            .kick(mac, Bans::MAX_KICK + Duration::from_secs(1), None)
            .await
            .is_err()
    );
    h.stop().await;
}

#[tokio::test]
async fn observe_mode_leaves_the_node_alone() {
    let mut h = Harness::start(false).await;
    let mac = h.up().await;
    h.engine.block_client(mac, true, None).unwrap();
    let kicked = h.handle().kick(mac, Duration::ZERO, None).await.unwrap();
    assert_eq!(kicked.nodes, 0);
    assert!(h.ban(QUIET).await.is_none());
    h.stop().await;
}

#[tokio::test]
async fn a_refusal_raises_a_condition_that_clears_when_the_node_takes_them() {
    let mut h = Harness::start(true).await;
    let mac = h.up().await;
    *h.refuse.lock().unwrap() = Some("Connection failed".to_owned());
    h.engine.block_client(mac, true, None).unwrap();
    h.ban(WAIT).await.unwrap();
    let failed = loop {
        let event = h.noted().await;
        if matches!(event.what, EventKind::ClientBansFailed { .. }) {
            break event;
        }
    };
    assert_eq!(
        failed.what,
        EventKind::ClientBansFailed {
            error: "Connection failed".to_owned()
        }
    );
    let condition = failed.condition.unwrap();
    assert_eq!(condition.key, format!("node:{}/client-bans", h.node));
    assert!(condition.raised);
    // It waits `retry` before asking again.
    assert!(h.ban(QUIET).await.is_none());

    *h.refuse.lock().unwrap() = None;
    h.clock.advance(Duration::from_mins(2));
    h.ban(WAIT).await.unwrap();
    let ok = h.noted().await;
    assert_eq!(ok.what, EventKind::ClientBansOk);
    assert!(!ok.condition.unwrap().raised);
    h.stop().await;
}
