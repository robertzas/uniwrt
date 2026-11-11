//! The schedules over a store and an engine watching the lab gateway's capture, its sections
//! renamed as render names a WLAN's (`uw_guest_2g` on radio0 through `phy1-ap0`, `uw_guest_5g`
//! on radio1 through `phy2-ap0`), the `bss` op answered by hand: the schedule's end turns both
//! off and is announced, the same request waits `retry`, a node in observe mode or held by a job
//! is left alone, and a refusal raises a condition that clears once the node takes it.

use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
    time::Duration,
};

use serde_json::{Value, json};
use tokio::sync::broadcast;
use uniwrt_core::{JobId, ManualClock, NodeId, Timestamp};
use uniwrt_model::{
    events::EventKind,
    job::{Job, JobKind, JobState},
    site::{
        SiteConfig,
        node::{ManagedPackage, NodeConfig},
        revision::{NewRevision, RevisionKind},
        wireless::{NodeScope, Schedule, Span, Weekday, Wlan, WlanSecurity},
    },
    wifi::Band,
};
use uniwrt_node::{Error, proto::ops::Set};
use uniwrt_store::{NodeRepo as _, RevisionRepo as _, Store};

use super::{SchedulesConfig, WlanSchedules};
use crate::{
    Engine, EngineConfig, Event,
    testing::{self, Call, Script, hello, identity, running},
};

/// Friday 2027-01-15, 08:00 UTC: 09:00 in Berlin, when the guest WLAN comes on.
const NOW: Timestamp = Timestamp::from_unix_secs(1_800_000_000);
/// 21:00 in Berlin the same day, when it goes off.
const EVENING: Timestamp = Timestamp::from_unix_secs(1_800_000_000 + 12 * 3_600);
const QUIET: Duration = Duration::from_millis(300);

struct Harness {
    _dir: tempfile::TempDir,
    clock: Arc<ManualClock>,
    engine: Engine<testing::SharedLink>,
    bus: broadcast::Receiver<Event>,
    jobs: broadcast::Sender<Arc<Job>>,
    schedules: Option<WlanSchedules>,
    node: NodeId,
    script: Script,
    /// What the `bss` op fails with, if it does.
    refuse: Arc<Mutex<Option<String>>>,
}

fn site(node: NodeId, managed: bool) -> SiteConfig {
    let mut config = SiteConfig::defaults(None);
    config.site.timezone = "Europe/Berlin".to_owned();
    config.wlans = vec![Wlan {
        id: "guest".to_owned(),
        ssid: "Guests".to_owned(),
        enabled: true,
        network: "lan".to_owned(),
        security: WlanSecurity::Open,
        passphrase: None,
        bands: vec![Band::G2, Band::G5],
        nodes: NodeScope::All,
        hidden: false,
        isolation: true,
        max_clients: None,
        schedule: Some(Schedule {
            spans: vec![Span {
                days: Weekday::ALL[..5].to_vec(),
                from: 9 * 60,
                to: 21 * 60,
            }],
        }),
        roaming: uniwrt_model::site::wireless::Roaming::default(),
    }];
    let mut node = NodeConfig::new(node);
    if managed {
        node.managed = BTreeSet::from([ManagedPackage::Wireless]);
    }
    config.nodes = vec![node];
    config
}

/// The capture's collect answer, its sections a WLAN's.
fn renamed(args: &Value) -> Value {
    let sets: Vec<Set> = serde_json::from_value(args["sets"].clone()).unwrap();
    let text = testing::collected(&sets)
        .to_string()
        .replace("default_radio0", "uw_guest_2g")
        .replace("default_radio1", "uw_guest_5g");
    serde_json::from_str(&text).unwrap()
}

impl Harness {
    async fn start(at: Timestamp, managed: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(ManualClock::new(at));
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
        store
            .commit_revision(
                NewRevision {
                    base: None,
                    kind: RevisionKind::Defaults,
                    restored_from: None,
                    author_id: None,
                    author: None,
                    message: "test".to_owned(),
                    config: site(node, managed),
                },
                Vec::new(),
            )
            .await
            .unwrap();
        let engine = Engine::new(EngineConfig::default(), clock.clone());
        let bus = engine.subscribe();
        let (jobs, held) = broadcast::channel(8);
        let config = SchedulesConfig {
            tick_every: Duration::from_millis(20),
            ..SchedulesConfig::default()
        };
        let schedules = WlanSchedules::start(&engine, store, Some(held), config);
        let (link, script) = testing::link();
        let refuse = Arc::new(Mutex::new(None::<String>));
        let refusal = Arc::clone(&refuse);
        script.answer(move |op, args| match op {
            "collect" => Some(Ok(renamed(args))),
            "bss" => Some(match refusal.lock().unwrap().clone() {
                Some(msg) => Err(Error::Remote {
                    op: "bss".to_owned(),
                    code: "not_found".to_owned(),
                    msg,
                }),
                None => Ok(json!({"radios": [{"radio": null, "on": [], "off": args["off"]}]})),
            }),
            _ => None,
        });
        engine.add(node, identity(), link, None).await;
        Self {
            _dir: dir,
            clock,
            engine,
            bus,
            jobs,
            schedules: Some(schedules),
            node,
            script,
            refuse,
        }
    }

    fn up(&self) {
        self.script.set_state(running(hello(true)));
    }

    /// The next `bss` request within `wait`, if any.
    async fn bss(&mut self, wait: Duration) -> Option<Call> {
        tokio::time::timeout(wait, self.script.call_of("bss"))
            .await
            .ok()
    }

    /// The next event noted of a WLAN schedule's kinds.
    async fn noted(&mut self) -> uniwrt_model::events::NewEvent {
        loop {
            match tokio::time::timeout(Duration::from_secs(10), self.bus.recv()).await {
                Ok(Ok(Event::Noted { event })) => return event,
                Ok(Ok(_) | Err(broadcast::error::RecvError::Lagged(_))) => {}
                other => panic!("no event noted: {other:?}"),
            }
        }
    }

    fn job(&self, state: JobState) {
        let job = Job {
            id: JobId::new(1),
            kind: JobKind::Apply,
            target: "site".to_owned(),
            node: Some(self.node),
            revision: None,
            state,
            steps: Vec::new(),
            nodes: Vec::new(),
            error: None,
            created_by: None,
            started_at: NOW,
            finished_at: None,
        };
        self.jobs.send(Arc::new(job)).unwrap();
    }

    async fn stop(mut self) {
        if let Some(schedules) = self.schedules.take() {
            schedules.stop().await;
        }
        self.engine.shutdown().await;
    }
}

fn args(call: &Call) -> (&str, Vec<&str>) {
    let phy = call.args["phy"].as_str().unwrap();
    let off = call.args["off"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i.as_str().unwrap())
        .collect();
    (phy, off)
}

#[tokio::test]
async fn the_schedules_end_turns_the_wlan_off_on_every_radio_and_is_announced() {
    let mut h = Harness::start(NOW, true).await;
    h.up();
    // On while its schedule has it on, and up: nothing to do.
    assert!(h.bss(QUIET).await.is_none());
    h.clock.set(EVENING);
    let first = h.bss(Duration::from_secs(5)).await.unwrap();
    let second = h.bss(Duration::from_secs(5)).await.unwrap();
    assert_eq!(args(&first), ("phy1", vec!["phy1-ap0"]));
    assert_eq!(args(&second), ("phy2", vec!["phy2-ap0"]));
    let event = h.noted().await;
    assert_eq!(
        event.what,
        EventKind::WlanScheduled {
            wlan: "guest".to_owned(),
            ssid: "Guests".to_owned(),
            on: false,
        }
    );
    assert_eq!(event.node, None);
    // The facts still show them up (the capture doesn't change): the same request waits.
    assert!(h.bss(QUIET).await.is_none());
    h.clock.advance(Duration::from_mins(11));
    let again = h.bss(Duration::from_secs(5)).await.unwrap();
    assert_eq!(args(&again).1, ["phy1-ap0"]);
    h.stop().await;
}

#[tokio::test]
async fn observe_mode_and_jobs_leave_the_node_alone() {
    let mut h = Harness::start(EVENING, false).await;
    h.up();
    assert!(h.bss(QUIET).await.is_none(), "observe mode");
    h.stop().await;

    let mut h = Harness::start(EVENING, true).await;
    h.job(JobState::Running);
    h.up();
    assert!(h.bss(QUIET).await.is_none(), "a job holds the node");
    h.job(JobState::Succeeded);
    let call = h.bss(Duration::from_secs(5)).await.unwrap();
    assert_eq!(args(&call), ("phy1", vec!["phy1-ap0"]));
    h.stop().await;
}

#[tokio::test]
async fn a_refusal_raises_a_condition_that_clears_when_the_node_takes_it() {
    let mut h = Harness::start(EVENING, true).await;
    *h.refuse.lock().unwrap() = Some("netifd has no hostapd config of phy1".to_owned());
    h.up();
    h.bss(Duration::from_secs(5)).await.unwrap();
    let event = h.noted().await;
    assert_eq!(
        event.what,
        EventKind::WlanScheduleFailed {
            phy: "phy1".to_owned(),
            error: "netifd has no hostapd config of phy1".to_owned(),
        }
    );
    assert_eq!(event.node, Some(h.node));
    let condition = event.condition.unwrap();
    assert_eq!(condition.key, format!("node:{}/wlan-schedule:phy1", h.node));
    assert!(condition.raised);

    *h.refuse.lock().unwrap() = None;
    h.clock.advance(Duration::from_mins(11));
    let ok = loop {
        let event = h.noted().await;
        if matches!(event.what, EventKind::WlanScheduleOk { .. }) {
            break event;
        }
    };
    assert_eq!(
        ok.what,
        EventKind::WlanScheduleOk {
            phy: "phy1".to_owned()
        }
    );
    assert!(!ok.condition.unwrap().raised);
    h.stop().await;
}
