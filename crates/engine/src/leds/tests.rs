//! The LEDs schedule over a store and an engine watching the lab gateway's capture, its `system`
//! set given LEDs by hand and the `leds` op answered by hand: lit LEDs go dark inside the
//! schedule and a held state comes back outside it, the same request waits `retry`, a node in
//! observe mode, left out or held by a job is left alone, and a refusal raises a condition that
//! clears once the node takes it.

use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
    time::Duration,
};

use serde_json::json;
use tokio::sync::broadcast;
use uniwrt_core::{JobId, ManualClock, NodeId, Timestamp};
use uniwrt_model::{
    events::EventKind,
    facts::LedFacts,
    job::{Job, JobKind, JobState},
    site::{
        SiteConfig,
        node::{ManagedPackage, NodeConfig},
        revision::{NewRevision, RevisionKind},
        schedules::Schedules,
        tz,
        wireless::{Schedule, Span, Weekday},
    },
};
use uniwrt_node::{Error, proto::ops::Set};
use uniwrt_store::{NodeRepo as _, RevisionRepo as _, Store};

use super::{LedSchedule, LedScheduleConfig, task::wanted};
use crate::{
    Engine, EngineConfig, Event,
    testing::{self, Call, Script, hello, identity, running},
};

/// Friday 2027-01-15, 08:00 UTC: 09:00 in Berlin, outside the schedule (22:00–07:00).
const DAY: Timestamp = Timestamp::from_unix_secs(1_800_000_000);
/// 23:00 in Berlin the same day, inside it.
const NIGHT: Timestamp = Timestamp::from_unix_secs(1_800_000_000 + 14 * 3_600);
const QUIET: Duration = Duration::from_millis(300);

const LIT: LedFacts = LedFacts {
    count: 3,
    lit: 3,
    held: false,
};
const DARK: LedFacts = LedFacts {
    count: 3,
    lit: 0,
    held: true,
};

/// How the node is set up in the site.
#[derive(Clone, Copy, PartialEq)]
enum Node {
    Managed,
    Observed,
    LeftOut,
}

struct Harness {
    _dir: tempfile::TempDir,
    clock: Arc<ManualClock>,
    engine: Engine<testing::SharedLink>,
    bus: broadcast::Receiver<Event>,
    jobs: broadcast::Sender<Arc<Job>>,
    schedule: Option<LedSchedule>,
    node: NodeId,
    script: Script,
    /// What the `leds` op fails with, if it does.
    refuse: Arc<Mutex<Option<String>>>,
}

fn site(node: NodeId, how: Node) -> SiteConfig {
    let mut config = SiteConfig::defaults(None);
    config.site.timezone = "Europe/Berlin".to_owned();
    config.schedules = Schedules {
        reboot: None,
        leds_off: Some(Schedule {
            spans: vec![Span {
                days: Weekday::ALL.to_vec(),
                from: 22 * 60,
                to: 7 * 60,
            }],
        }),
    };
    let mut entry = NodeConfig::new(node);
    if how != Node::Observed {
        entry.managed = BTreeSet::from([ManagedPackage::System]);
    }
    entry.led_schedule = how != Node::LeftOut;
    config.nodes = vec![entry];
    config
}

impl Harness {
    async fn start(at: Timestamp, how: Node, leds: LedFacts) -> Self {
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
                    config: site(node, how),
                },
                Vec::new(),
            )
            .await
            .unwrap();
        let engine = Engine::new(EngineConfig::default(), clock.clone());
        let bus = engine.subscribe();
        let (jobs, held) = broadcast::channel(8);
        let config = LedScheduleConfig {
            tick_every: Duration::from_millis(20),
            ..LedScheduleConfig::default()
        };
        let schedule = LedSchedule::start(&engine, store, Some(held), config);
        let (link, script) = testing::link();
        let refuse = Arc::new(Mutex::new(None::<String>));
        let refusal = Arc::clone(&refuse);
        script.answer(move |op, args| match op {
            "collect" => {
                let sets: Vec<Set> = serde_json::from_value(args["sets"].clone()).unwrap();
                let mut answer = testing::collected(&sets);
                if let Some(system) = answer["sets"].get_mut("system") {
                    system["leds"] = serde_json::to_value(leds).unwrap();
                }
                Some(Ok(answer))
            }
            "leds" => Some(match refusal.lock().unwrap().clone() {
                Some(msg) => Err(Error::Remote {
                    op: "leds".to_owned(),
                    code: "failed".to_owned(),
                    msg,
                }),
                None => Ok(json!({"count": 3, "lit": 0, "held": true})),
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
            schedule: Some(schedule),
            node,
            script,
            refuse,
        }
    }

    fn up(&self) {
        self.script.set_state(running(hello(true)));
    }

    /// Whether the next `leds` request within `wait` turns them off, if one comes.
    async fn leds(&mut self, wait: Duration) -> Option<bool> {
        let call: Call = tokio::time::timeout(wait, self.script.call_of("leds"))
            .await
            .ok()?;
        Some(call.args["off"].as_bool().unwrap())
    }

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
            started_at: DAY,
            finished_at: None,
        };
        self.jobs.send(Arc::new(job)).unwrap();
    }

    async fn stop(mut self) {
        if let Some(schedule) = self.schedule.take() {
            schedule.stop().await;
        }
        self.engine.shutdown().await;
    }
}

#[test]
fn only_what_differs_from_the_schedule_is_wanted() {
    let node = NodeId::new(1);
    let local = |minute| tz::Local {
        day: Weekday::Fri,
        minute,
        offset_s: 0,
    };
    let config = site(node, Node::Managed);
    let night = local(23 * 60);
    let day = local(9 * 60);
    assert_eq!(wanted(&config, node, &LIT, night), Some(true));
    assert_eq!(wanted(&config, node, &DARK, night), None);
    assert_eq!(wanted(&config, node, &DARK, day), Some(false));
    assert_eq!(wanted(&config, node, &LIT, day), None);
    // Partly lit with a state held (a hotplug lit one again): dark again at night.
    let partly = LedFacts { lit: 1, ..DARK };
    assert_eq!(wanted(&config, node, &partly, night), Some(true));
    let unlit = LedFacts::default();
    assert_eq!(wanted(&config, node, &unlit, night), None);
    for how in [Node::Observed, Node::LeftOut] {
        assert_eq!(wanted(&site(node, how), node, &LIT, night), None);
    }
    // Left out while dark: back on, whatever the time.
    assert_eq!(
        wanted(&site(node, Node::LeftOut), node, &DARK, night),
        Some(false)
    );
}

#[tokio::test]
async fn lit_leds_go_dark_at_night_and_the_same_request_waits() {
    let mut h = Harness::start(DAY, Node::Managed, LIT).await;
    h.up();
    assert_eq!(h.leds(QUIET).await, None, "lit by day: nothing to do");
    h.clock.set(NIGHT);
    assert_eq!(h.leds(Duration::from_secs(5)).await, Some(true));
    // The facts still show them lit (the capture doesn't change): the same request waits.
    assert_eq!(h.leds(QUIET).await, None);
    h.clock.advance(Duration::from_mins(11));
    assert_eq!(h.leds(Duration::from_secs(5)).await, Some(true));
    h.stop().await;
}

#[tokio::test]
async fn a_held_state_comes_back_in_the_morning() {
    let mut h = Harness::start(NIGHT, Node::Managed, DARK).await;
    h.up();
    assert_eq!(h.leds(QUIET).await, None, "dark at night: nothing to do");
    h.clock
        .set(Timestamp::from_unix_secs(1_800_000_000 + 24 * 3_600));
    assert_eq!(h.leds(Duration::from_secs(5)).await, Some(false));
    h.stop().await;
}

#[tokio::test]
async fn observe_mode_a_node_left_out_and_jobs_leave_the_leds_alone() {
    for how in [Node::Observed, Node::LeftOut] {
        let mut h = Harness::start(NIGHT, how, LIT).await;
        h.up();
        assert_eq!(h.leds(QUIET).await, None);
        h.stop().await;
    }

    let mut h = Harness::start(NIGHT, Node::Managed, LIT).await;
    h.job(JobState::Running);
    h.up();
    assert_eq!(h.leds(QUIET).await, None, "a job holds the node");
    h.job(JobState::Succeeded);
    assert_eq!(h.leds(Duration::from_secs(5)).await, Some(true));
    h.stop().await;
}

#[tokio::test]
async fn a_refusal_raises_a_condition_that_clears_when_the_node_takes_it() {
    let mut h = Harness::start(NIGHT, Node::Managed, LIT).await;
    *h.refuse.lock().unwrap() = Some("cannot turn off wan: Permission denied".to_owned());
    h.up();
    h.leds(Duration::from_secs(5)).await.unwrap();
    let event = h.noted().await;
    assert_eq!(
        event.what,
        EventKind::LedScheduleFailed {
            error: "cannot turn off wan: Permission denied".to_owned(),
        }
    );
    assert_eq!(event.node, Some(h.node));
    let condition = event.condition.unwrap();
    assert_eq!(condition.key, format!("node:{}/led-schedule", h.node));
    assert!(condition.raised);

    *h.refuse.lock().unwrap() = None;
    h.clock.advance(Duration::from_mins(11));
    let ok = loop {
        let event = h.noted().await;
        if event.what == EventKind::LedScheduleOk {
            break event;
        }
    };
    assert!(!ok.condition.unwrap().raised);
    h.stop().await;
}
