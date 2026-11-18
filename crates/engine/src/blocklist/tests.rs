//! The blocklists over a store, an engine watching the lab gateway's capture and a scripted
//! list source; the gateway's `/tmp` is kept by hand from what is uploaded and run, and reported
//! in its `services` set: a gateway without the list gets it and dnsmasq restarted, a changed
//! list is signalled (restarted when the signal finds no dnsmasq), a failed push is reported and tried again, a gateway the controller
//! doesn't manage is left alone, and blocking off removes the files.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::{Arc, Mutex},
    time::Duration,
};

use bytes::Bytes;
use serde_json::{Value, json};
use uniwrt_core::{JobId, MacAddr, ManualClock, NodeId, Timestamp};
use uniwrt_dns::{Blocklists, BoxFuture, Fetched, Source, Validators};
use uniwrt_model::{
    apply::{Applied, AppliedNode},
    blocklists::{BlocklistState, SyncState},
    node::NodeRole,
    site::{
        SiteConfig,
        blocking::{BLOCKLIST_CONF, BLOCKLIST_FILE, CustomList},
        node::{ManagedPackage, NodeConfig},
        revision::{NewRevision, RevisionKind},
    },
};
use uniwrt_node::proto::ops::Set;
use uniwrt_store::{ApplyRepo as _, NodeRepo as _, RevisionRepo as _, Store};

use super::{BlocklistConfig, BlocklistError, BlocklistSync, task::due};
use crate::{
    Engine, EngineConfig, Identity,
    testing::{self, Call, Script, hello, running},
};

const NOW: Timestamp = Timestamp::from_unix_secs(1_800_000_000);
const QUIET: Duration = Duration::from_millis(300);
const WAIT: Duration = Duration::from_secs(5);
const URL: &str = "https://lists.test/ads.txt";

/// The lists' bodies by URL.
#[derive(Default)]
struct FakeSource(Mutex<HashMap<String, Bytes>>);

impl Source for FakeSource {
    fn fetch<'a>(
        &'a self,
        url: &'a str,
        _validators: &'a Validators,
    ) -> BoxFuture<'a, Result<Fetched, uniwrt_dns::Error>> {
        let body = self.0.lock().unwrap().get(url).cloned();
        Box::pin(async move {
            body.map(|body| Fetched::Body {
                body,
                validators: Validators::default(),
            })
            .ok_or_else(|| uniwrt_dns::Error::Status {
                url: url.to_owned(),
                status: 404,
            })
        })
    }
}

/// The gateway's `/tmp` files, and how its commands answer.
#[derive(Default)]
struct Gateway {
    files: BTreeMap<String, String>,
    /// The exit code and stderr the next command named here fails with.
    fail: Option<(String, u32)>,
}

impl Gateway {
    fn services(&self) -> Value {
        let head = self
            .files
            .get(BLOCKLIST_FILE)
            .map(|f| f.lines().next().unwrap_or_default());
        json!({
            "ddns": [], "doh": null,
            "blocklist": head, "blocklist_conf": self.files.contains_key(BLOCKLIST_CONF),
        })
    }

    fn answer(&mut self, op: &str, args: &Value) -> Option<uniwrt_node::Result<Value>> {
        match op {
            "collect" => {
                let sets: Vec<Set> = serde_json::from_value(args["sets"].clone()).unwrap();
                let mut answer = testing::collected(&sets);
                if sets.contains(&Set::Services) {
                    answer["sets"]["services"] = self.services();
                }
                Some(Ok(answer))
            }
            "upload" => {
                let path = args["path"].as_str().unwrap().to_owned();
                self.files
                    .insert(path, args["data"].as_str().unwrap().to_owned());
                Some(Ok(json!({})))
            }
            "exec" => {
                let words: Vec<String> = serde_json::from_value(args.clone()).unwrap();
                if let Some((command, code)) = self.fail.take_if(|(c, _)| *c == words[0]) {
                    return Some(Ok(
                        json!({"code": code, "stderr": format!("{command}: no")}),
                    ));
                }
                if words[0] == "rm" {
                    for path in &words[2..] {
                        self.files.remove(path);
                    }
                }
                Some(Ok(json!({"code": 0})))
            }
            _ => None,
        }
    }
}

struct Harness {
    _dir: tempfile::TempDir,
    clock: Arc<ManualClock>,
    engine: Engine<testing::SharedLink>,
    sync: Option<BlocklistSync>,
    node: NodeId,
    script: Script,
    source: Arc<FakeSource>,
    gateway: Arc<Mutex<Gateway>>,
}

impl Harness {
    /// A gateway whose applied revision has blocking `on` (one list and an own block), managing
    /// its DHCP and DNS when `managed`.
    async fn start(managed: bool, on: bool, gateway: Gateway) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(ManualClock::new(NOW));
        let store = Store::open(dir.path().join("test.db"), clock.clone())
            .await
            .unwrap();
        let mac = MacAddr::new([2, 0, 0, 0, 0, 1]);
        let node = store
            .create_node(uniwrt_model::node::NewNode {
                name: "gw".to_owned(),
                mac,
                board_name: "x".to_owned(),
                host: "10.0.0.1".to_owned(),
                port: 22,
                role: NodeRole::Gateway,
            })
            .await
            .unwrap()
            .id;
        let mut config = SiteConfig::defaults(None);
        let mut node_config = NodeConfig::new(node);
        if managed {
            node_config.managed = BTreeSet::from([ManagedPackage::Dhcp]);
        }
        config.nodes = vec![node_config];
        let blocking = &mut config.services.blocking;
        blocking.enabled = on;
        blocking.lists.clear();
        blocking.custom = vec![CustomList {
            name: "Ads".to_owned(),
            url: URL.to_owned(),
        }];
        blocking.deny = vec!["own.example".to_owned()];
        let revision = NewRevision {
            base: None,
            kind: RevisionKind::Defaults,
            restored_from: None,
            author_id: None,
            author: None,
            message: "test".to_owned(),
            config,
        };
        let revision = store.commit_revision(revision, Vec::new()).await.unwrap();
        let applied = Applied {
            revision: revision.id,
            job: JobId::new(1),
            nodes: vec![AppliedNode {
                node,
                holds: true,
                host: None,
                packages: BTreeMap::new(),
            }],
        };
        store.record_applied(applied).await.unwrap();
        let source = Arc::new(FakeSource::default());
        source
            .0
            .lock()
            .unwrap()
            .insert(URL.to_owned(), Bytes::from_static(b"0.0.0.0 ads.example\n"));
        let lists = Blocklists::open(dir.path().join("lists"), source.clone())
            .await
            .unwrap();
        let engine = Engine::new(EngineConfig::default(), clock.clone());
        let config = BlocklistConfig {
            tick_every: Duration::from_millis(20),
            repush: Duration::from_secs(60),
            timeout: WAIT,
            ..BlocklistConfig::default()
        };
        let sync = BlocklistSync::start(&engine, store, lists, None, config);
        let (link, script) = testing::link();
        let gateway = Arc::new(Mutex::new(gateway));
        let answers = Arc::clone(&gateway);
        script.answer(move |op, args| answers.lock().unwrap().answer(op, args));
        let identity = Identity {
            role: NodeRole::Gateway,
            mac,
        };
        engine.add(node, identity, link, None).await;
        Self {
            _dir: dir,
            clock,
            engine,
            sync: Some(sync),
            node,
            script,
            source,
            gateway,
        }
    }

    fn state(&self) -> Arc<BlocklistState> {
        self.sync.as_ref().unwrap().handle().state()
    }

    /// Waits until the state is as `test` wants it.
    async fn until(
        &self,
        what: &str,
        test: impl Fn(&BlocklistState) -> bool,
    ) -> Arc<BlocklistState> {
        let deadline = tokio::time::Instant::now() + WAIT;
        loop {
            let state = self.state();
            if test(&state) {
                return state;
            }
            assert!(tokio::time::Instant::now() < deadline, "{what}: {state:#?}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// The next push request (`exec` or `upload`), skipping the agent's ops.
    async fn next(&mut self) -> Call {
        tokio::time::timeout(WAIT, async {
            loop {
                let call = self.script.call().await;
                if call.op == "exec" || call.op == "upload" {
                    return call;
                }
            }
        })
        .await
        .expect("a push request")
    }

    /// No push request within `QUIET`.
    async fn no_push(&mut self) {
        let quiet = tokio::time::timeout(QUIET, async {
            loop {
                let call = self.script.call().await;
                if call.op == "exec" || call.op == "upload" {
                    return call;
                }
            }
        });
        if let Ok(call) = quiet.await {
            panic!("unexpected push {call:?}");
        }
    }

    /// The gateway reports its files now (as its next `services` set would).
    fn report(&self) {
        assert!(self.engine.recollect(self.node));
    }

    async fn stop(mut self) {
        if let Some(sync) = self.sync.take() {
            sync.stop().await;
        }
        self.engine.shutdown().await;
    }
}

fn argv(call: &Call) -> Vec<String> {
    assert_eq!(call.op, "exec", "{call:?}");
    serde_json::from_value(call.args.clone()).unwrap()
}

fn uploaded(call: &Call) -> (String, String) {
    assert_eq!(call.op, "upload", "{call:?}");
    let text = |key: &str| call.args[key].as_str().unwrap().to_owned();
    (text("path"), text("data"))
}

#[tokio::test]
async fn a_gateway_without_the_list_gets_it_then_a_changed_list_is_signalled() {
    let mut h = Harness::start(true, true, Gateway::default()).await;
    h.script.set_state(running(hello(true)));
    assert_eq!(
        argv(&h.next().await),
        ["mkdir", "-p", "/tmp/dnsmasq.uniwrt.d"]
    );
    let (path, list) = uploaded(&h.next().await);
    assert_eq!(path, BLOCKLIST_FILE);
    let lines: Vec<&str> = list.lines().collect();
    assert!(lines[0].starts_with("# uniwrt "), "{list}");
    assert_eq!(lines[1..], ["server=/ads.example/", "server=/own.example/"]);
    let (path, conf) = uploaded(&h.next().await);
    assert_eq!(path, BLOCKLIST_CONF);
    assert_eq!(conf, format!("servers-file={BLOCKLIST_FILE}\n"));
    assert_eq!(argv(&h.next().await), ["/etc/init.d/dnsmasq", "restart"]);

    // Until the gateway reports it, it isn't sent again.
    h.no_push().await;
    h.report();
    let state = h
        .until("synced", |s| s.sync.state == SyncState::Synced)
        .await;
    let compiled = state.compiled.clone().unwrap();
    assert_eq!((compiled.domains, compiled.cut), (2, 0));
    assert_eq!(state.sync.sha256.as_ref(), Some(&compiled.sha256));
    assert_eq!(state.lists.len(), 1);
    assert_eq!(state.lists[0].domains, Some(1));
    assert!(state.enabled && state.sync.pushed_at.is_some());

    // A refreshed list that changed is pushed and signalled: the conf file is in place.
    h.source.0.lock().unwrap().insert(
        URL.to_owned(),
        Bytes::from_static(b"0.0.0.0 ads.example\n0.0.0.0 more.example\n"),
    );
    h.sync.as_ref().unwrap().handle().refresh().await.unwrap();
    assert_eq!(argv(&h.next().await)[0], "mkdir");
    let (path, list) = uploaded(&h.next().await);
    assert_eq!(path, BLOCKLIST_FILE);
    assert!(list.contains("server=/more.example/"), "{list}");
    let signal = argv(&h.next().await);
    assert_eq!(signal[..4], ["ubus", "call", "service", "signal"]);
    assert_eq!(signal[4], r#"{"name":"dnsmasq","signal":1}"#);
    h.no_push().await;
    h.report();
    h.until("synced again", |s| s.sync.state == SyncState::Synced)
        .await;

    // A dnsmasq that doesn't run can't be signalled (procd: "Not found"): it is restarted.
    h.source.0.lock().unwrap().insert(
        URL.to_owned(),
        Bytes::from_static(b"0.0.0.0 ads.example\n0.0.0.0 most.example\n"),
    );
    h.gateway.lock().unwrap().fail = Some(("ubus".to_owned(), 4));
    h.sync.as_ref().unwrap().handle().refresh().await.unwrap();
    assert_eq!(argv(&h.next().await)[0], "mkdir");
    assert_eq!(uploaded(&h.next().await).0, BLOCKLIST_FILE);
    assert_eq!(argv(&h.next().await)[0], "ubus");
    assert_eq!(argv(&h.next().await), ["/etc/init.d/dnsmasq", "restart"]);
    h.report();
    let state = h.until("synced again", |s| {
        s.sync.state == SyncState::Synced && s.compiled.as_ref().is_some_and(|c| c.domains == 3)
    });
    assert_ne!(
        state.await.compiled.as_ref().unwrap().sha256,
        compiled.sha256
    );
    h.stop().await;
}

#[tokio::test]
async fn a_failed_push_is_reported_and_tried_again() {
    let gateway = Gateway {
        fail: Some(("/etc/init.d/dnsmasq".to_owned(), 1)),
        ..Gateway::default()
    };
    let mut h = Harness::start(true, true, gateway).await;
    h.script.set_state(running(hello(true)));
    for _ in 0..4 {
        h.next().await;
    }
    let state = h
        .until("failed", |s| s.sync.state == SyncState::Failed)
        .await;
    let error = state.sync.error.as_deref().unwrap();
    assert!(
        error.contains("/etc/init.d/dnsmasq restart failed"),
        "{error}"
    );
    h.no_push().await;
    h.clock.advance(Duration::from_secs(61));
    // Tried again whole: the gateway hasn't reported the files it took.
    assert_eq!(argv(&h.next().await)[0], "mkdir");
    assert_eq!(uploaded(&h.next().await).0, BLOCKLIST_FILE);
    assert_eq!(uploaded(&h.next().await).0, BLOCKLIST_CONF);
    assert_eq!(argv(&h.next().await), ["/etc/init.d/dnsmasq", "restart"]);
    h.report();
    h.until("synced", |s| s.sync.state == SyncState::Synced)
        .await;
    assert_eq!(h.state().sync.error, None);
    h.stop().await;
}

#[tokio::test]
async fn an_unmanaged_gateway_is_left_alone() {
    let mut h = Harness::start(false, true, Gateway::default()).await;
    h.script.set_state(running(hello(true)));
    h.until("unmanaged", |s| s.sync.state == SyncState::Unmanaged)
        .await;
    h.no_push().await;
    // Its lists are still refreshed for the saved revision.
    let state = h.until("refreshed", |s| s.refreshed_at.is_some()).await;
    assert_eq!(state.lists[0].domains, Some(1));
    assert_eq!(state.compiled, None);
    h.stop().await;
}

#[tokio::test]
async fn blocking_off_removes_the_files_and_refreshes_nothing() {
    let mut files = BTreeMap::new();
    files.insert(
        BLOCKLIST_FILE.to_owned(),
        format!("# uniwrt {} 1\n", "a".repeat(64)),
    );
    files.insert(BLOCKLIST_CONF.to_owned(), String::new());
    let gateway = Gateway {
        files,
        ..Gateway::default()
    };
    let mut h = Harness::start(true, false, gateway).await;
    h.script.set_state(running(hello(true)));
    assert_eq!(
        argv(&h.next().await),
        ["rm", "-f", BLOCKLIST_CONF, BLOCKLIST_FILE]
    );
    h.report();
    h.until("off", |s| {
        s.sync.state == SyncState::Off && s.sync.sha256.is_none()
    })
    .await;
    h.no_push().await;
    assert!(h.gateway.lock().unwrap().files.is_empty());
    let refresh = h.sync.as_ref().unwrap().handle().refresh().await;
    assert!(matches!(refresh, Err(BlocklistError::Off)), "{refresh:?}");
    assert_eq!(h.state().lists, []);
    h.stop().await;
}

#[test]
fn lists_refresh_nightly_when_stale_and_at_any_time_when_overdue() {
    let config = BlocklistConfig::default();
    let at = |hours: u64| Some(NOW.saturating_sub(Duration::from_hours(hours)));
    assert!(due(None, NOW, false, &config), "never asked for");
    assert!(!due(at(19), NOW, true, &config));
    assert!(due(at(21), NOW, true, &config));
    assert!(!due(at(21), NOW, false, &config));
    assert!(due(at(49), NOW, false, &config));
}
