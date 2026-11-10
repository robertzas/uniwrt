//! The drift watcher over a store and an engine watching a scripted node whose `/etc/config`
//! the test holds: a semantic change is drift (acknowledged, changed again, reverted), a change
//! in form only is confirmed as it reads now, a muted package is left alone, a gone file is a
//! removed drift, and a job holds the node until it ends and the apply confirms it anew.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use serde_json::{Value, json};
use tokio::sync::broadcast;
use uniwrt_core::{JobId, ManualClock, NodeId, Timestamp};
use uniwrt_model::{
    apply::{Applied, AppliedNode},
    drift::{DriftChange, DriftEnd, DriftState},
    job::{Job, JobKind, JobState},
    node::{NewNode, NodeRole},
    site::{
        SiteConfig,
        revision::{NewRevision, RevisionKind},
    },
};
use uniwrt_node::Error;
use uniwrt_store::{ApplyRepo as _, DriftRepo as _, NodeRepo as _, RevisionRepo as _, Store};

use super::{DriftConfig, DriftWatch, Drifts, task_sha256};
use crate::{
    Engine, EngineConfig, Event,
    testing::{self, Script, SharedLink, hello, identity, running},
};

const NOW: Timestamp = Timestamp::from_unix_secs(1_800_000_000);
const NETWORK: &str =
    "\nconfig interface 'lan'\n\toption proto 'static'\n\toption ipaddr '192.168.1.1/24'\n";
const DHCP: &str = "\nconfig dnsmasq\n\toption domain 'lan'\n";

type Files = Arc<Mutex<BTreeMap<String, String>>>;

struct Harness {
    _dir: tempfile::TempDir,
    store: Store,
    engine: Engine<SharedLink>,
    bus: broadcast::Receiver<Event>,
    jobs: broadcast::Sender<Arc<Job>>,
    watch: DriftWatch,
    drifts: Drifts,
    node: NodeId,
    script: Script,
    files: Files,
}

impl Harness {
    async fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(ManualClock::new(NOW));
        let store = Store::open(dir.path().join("test.db"), clock.clone())
            .await
            .unwrap();
        let node = store
            .create_node(NewNode {
                name: "ap".to_owned(),
                mac: identity().mac,
                board_name: "x".to_owned(),
                host: "10.0.0.2".to_owned(),
                port: 22,
                role: NodeRole::Ap,
            })
            .await
            .unwrap()
            .id;
        let files: Files = Arc::new(Mutex::new(BTreeMap::from([
            ("network".to_owned(), NETWORK.to_owned()),
            ("dhcp".to_owned(), DHCP.to_owned()),
        ])));
        apply(&store, node, &files).await;
        let engine = Engine::new(EngineConfig::default(), clock);
        let bus = engine.subscribe();
        let (jobs, _) = broadcast::channel(8);
        let config = DriftConfig {
            tick_every: Duration::from_secs(3600),
            read_timeout: Duration::from_secs(5),
        };
        let watch = DriftWatch::start(&engine, store.clone(), Some(jobs.subscribe()), config)
            .await
            .unwrap();
        let drifts = watch.handle();
        let (link, script) = testing::link();
        let served = Arc::clone(&files);
        script.answer(move |op, args| answer(&served, op, args));
        engine.add(node, identity(), link, None).await;
        script.set_state(running(hello(true)));
        let mut bus = bus;
        // The agent's first collect: the actor listens to its pushes from then on.
        let first = async { while !matches!(bus.recv().await.unwrap(), Event::Facts { .. }) {} };
        tokio::time::timeout(Duration::from_secs(10), first)
            .await
            .expect("the first facts");
        Self {
            _dir: dir,
            store,
            engine,
            bus,
            jobs,
            watch,
            drifts,
            node,
            script,
            files,
        }
    }

    /// Writes a file on the node and has the agent report the hashes.
    fn write(&self, package: &str, text: Option<&str>) {
        {
            let mut files = self.files.lock().unwrap();
            match text {
                Some(text) => files.insert(package.to_owned(), text.to_owned()),
                None => files.remove(package),
            };
        }
        self.script
            .push_tick(&json!({"sets": {"config": hashes(&self.files)}, "errors": {}}));
    }

    /// The next drift announcement that `want` accepts.
    async fn drift(
        &mut self,
        want: impl Fn(&DriftState, &[DriftChange]) -> bool,
    ) -> (Arc<DriftState>, Vec<DriftChange>) {
        let search = async {
            loop {
                if let Event::Drift { state, changes } = self.bus.recv().await.unwrap()
                    && want(&state, &changes)
                {
                    return (state, changes.to_vec());
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(10), search)
            .await
            .expect("drift within 10 s")
    }

    /// No file read or hashes asked for within `wait`.
    async fn no_read(&mut self, wait: Duration) {
        let reads = async {
            loop {
                let call = self.script.call().await;
                if call.op == "read_file" || call.args == json!({"sets": ["config"]}) {
                    return call;
                }
            }
        };
        if let Ok(call) = tokio::time::timeout(wait, reads).await {
            panic!("unexpected request {call:?}");
        }
    }

    fn ended(&self, how: DriftEnd) -> DriftChange {
        DriftChange::Ended {
            node: self.node,
            package: "network".to_owned(),
            how,
        }
    }
}

/// Records an apply that leaves the node holding `files`.
async fn apply(store: &Store, node: NodeId, files: &Files) {
    let packages = files.lock().unwrap().clone();
    let head = store.head_revision().await.unwrap();
    let revision = store
        .commit_revision(
            NewRevision {
                base: head.map(|head| head.meta.id),
                kind: RevisionKind::Defaults,
                restored_from: None,
                author_id: None,
                author: None,
                message: "test".to_owned(),
                config: SiteConfig::defaults(None),
            },
            Vec::new(),
        )
        .await
        .unwrap();
    store
        .record_applied(Applied {
            revision: revision.id,
            job: JobId::new(1),
            nodes: vec![AppliedNode {
                holds: true,
                node,
                host: None,
                packages,
            }],
        })
        .await
        .unwrap();
}

fn hashes(files: &Files) -> Value {
    let files: serde_json::Map<_, _> = files
        .lock()
        .unwrap()
        .iter()
        .map(|(name, text)| (name.clone(), Value::String(task_sha256(text))))
        .collect();
    json!({ "files": files })
}

fn answer(files: &Files, op: &str, args: &Value) -> Option<uniwrt_node::Result<Value>> {
    match op {
        "read_file" => {
            let path = args["path"].as_str().unwrap_or_default();
            let name = path.strip_prefix("/etc/config/").unwrap_or_default();
            Some(match files.lock().unwrap().get(name) {
                Some(data) => Ok(json!({"data": data, "truncated": false})),
                None => Err(Error::Remote {
                    op: op.to_owned(),
                    code: "not_found".to_owned(),
                    msg: format!("{path}: No such file or directory"),
                }),
            })
        }
        "collect" if args["sets"] == json!(["config"]) => {
            Some(Ok(json!({"sets": {"config": hashes(files)}, "errors": {}})))
        }
        _ => None,
    }
}

fn job(id: i64, node: NodeId, state: JobState) -> Arc<Job> {
    Arc::new(Job {
        id: JobId::new(id),
        kind: JobKind::Apply,
        target: "site".to_owned(),
        node: Some(node),
        revision: None,
        state,
        steps: Vec::new(),
        nodes: Vec::new(),
        error: None,
        created_by: None,
        started_at: NOW,
        finished_at: (state != JobState::Running).then_some(NOW),
    })
}

const EDITED: &str =
    "\nconfig interface 'lan'\n\toption proto 'static'\n\toption ipaddr '192.168.7.1/24'\n";

#[tokio::test]
async fn a_change_is_drift_until_reverted_and_an_acknowledgement_lasts_until_the_next() {
    let mut h = Harness::start().await;
    h.write("network", Some(EDITED));
    let (state, changes) = h.drift(|_, changes| !changes.is_empty()).await;
    assert!(matches!(
        &changes[..],
        [DriftChange::Detected { package, removed: false, .. }] if package == "network"
    ));
    let drift = state.drift(h.node, "network").unwrap();
    assert_eq!(drift.diff.sections.len(), 1);
    assert_eq!(drift.diff.sections[0].options[0].name, "ipaddr");
    assert_eq!(h.store.drifts().await.unwrap().len(), 1);

    h.drifts
        .acknowledge(h.node, "network".to_owned(), Some("admin".to_owned()))
        .await
        .unwrap();
    let (state, changes) = h.drift(|_, changes| !changes.is_empty()).await;
    assert_eq!(changes, [h.ended(DriftEnd::Acknowledged)]);
    let ack = state.drift(h.node, "network").unwrap().acknowledged.clone();
    assert_eq!(ack.unwrap().by.as_deref(), Some("admin"));
    assert!(
        h.drifts
            .acknowledge(h.node, "dhcp".to_owned(), None)
            .await
            .is_err()
    );

    // A further change is drift again.
    h.write("network", Some(&EDITED.replace("7.1", "8.1")));
    let (state, _) = h
        .drift(|_, changes| matches!(changes, [DriftChange::Detected { .. }]))
        .await;
    assert!(
        state
            .drift(h.node, "network")
            .unwrap()
            .acknowledged
            .is_none()
    );

    h.write("network", Some(NETWORK));
    let (state, changes) = h.drift(|_, changes| !changes.is_empty()).await;
    assert_eq!(changes, [h.ended(DriftEnd::Reverted)]);
    assert_eq!(state.drifts, []);
    assert_eq!(h.store.drifts().await.unwrap(), []);
    h.watch.stop().await;
}

#[tokio::test]
async fn a_change_in_form_only_is_confirmed_as_it_reads() {
    let mut h = Harness::start().await;
    let reformatted = NETWORK.replace("'static'", "static");
    h.write("network", Some(&reformatted));
    h.script.call_of("read_file").await;
    // Wait for the store to catch up: the confirmed row takes the new form.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let confirmed = h.store.confirmed(Some(h.node)).await.unwrap();
        let network = confirmed.iter().find(|c| c.package == "network").unwrap();
        if network.text == reformatted {
            assert_eq!(network.sha256, task_sha256(&reformatted));
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "not reconfirmed");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(h.drifts.state().drifts, []);
    h.watch.stop().await;
}

#[tokio::test]
async fn a_muted_package_is_left_alone_until_watched_again() {
    let mut h = Harness::start().await;
    h.drifts
        .mute(h.node, "network".to_owned(), true, Some("admin".to_owned()))
        .await
        .unwrap();
    let (state, _) = h.drift(|state, _| !state.muted.is_empty()).await;
    assert!(state.is_muted(h.node, "network"));
    h.script.drain();
    h.write("network", Some(EDITED));
    h.no_read(Duration::from_millis(300)).await;
    assert_eq!(h.drifts.state().drifts, []);

    h.drifts
        .mute(h.node, "network".to_owned(), false, None)
        .await
        .unwrap();
    let (state, _) = h
        .drift(|_, changes| matches!(changes, [DriftChange::Detected { .. }]))
        .await;
    assert_eq!(state.muted, []);
    assert_eq!(h.store.drift_mutes().await.unwrap(), []);
    h.watch.stop().await;
}

#[tokio::test]
async fn a_gone_file_is_a_removed_drift() {
    let mut h = Harness::start().await;
    h.write("dhcp", None);
    let (state, _) = h.drift(|_, changes| !changes.is_empty()).await;
    let drift = state.drift(h.node, "dhcp").unwrap();
    assert!(drift.removed);
    assert_eq!(drift.diff.sections.len(), 1);
    h.watch.stop().await;
}

#[tokio::test]
async fn a_job_holds_the_node_and_its_apply_confirms_the_package_anew() {
    let mut h = Harness::start().await;
    h.write("network", Some(EDITED));
    h.drift(|_, changes| !changes.is_empty()).await;

    h.jobs.send(job(5, h.node, JobState::Running)).unwrap();
    // Wait until the watcher took the job in (its loop serves jobs before commands).
    h.drifts
        .mute(h.node, "x".to_owned(), true, None)
        .await
        .unwrap();
    h.script.drain();
    h.write("network", Some(&EDITED.replace("7.1", "9.1")));
    h.no_read(Duration::from_millis(300)).await;

    // The apply overwrote the drift: the node holds what it confirmed.
    h.files
        .lock()
        .unwrap()
        .insert("network".to_owned(), NETWORK.replace("1.1", "2.1"));
    apply(&h.store, h.node, &h.files).await;
    h.jobs.send(job(5, h.node, JobState::Succeeded)).unwrap();
    let (state, changes) = h.drift(|_, changes| !changes.is_empty()).await;
    assert_eq!(changes, [h.ended(DriftEnd::Confirmed)]);
    assert_eq!(state.drifts, []);
    // The watcher asked for fresh hashes rather than reading the files.
    assert_eq!(
        h.script.call_of("collect").await.args,
        json!({"sets": ["config"]})
    );
    h.no_read(Duration::from_millis(300)).await;
    drop(h.engine.clone());
    h.watch.stop().await;
}
