//! The recorder over a store and an engine without nodes, fed by hand on a manual clock: the
//! controller's start, a node going offline (once it lasts) and back after a restart, log
//! entries whose pattern is an event folding together, and an adoption from the job runner.

use std::{sync::Arc, time::Duration};

use tokio::sync::broadcast;
use uniwrt_core::{Clock as _, JobId, LogId, ManualClock, NodeId, Timestamp};
use uniwrt_model::{
    events::{EventFilter, EventKind, EventRecord},
    job::{Job, JobKind, JobState},
    log::{LogEntry, LogRecord, LogVia, Severity},
    status::{NodeState, NodeStatus},
};
use uniwrt_store::{EventRepo as _, Store};

use super::{EventRecorder, EventsConfig};
use crate::{Engine, EngineConfig, Event};

const NODE: NodeId = NodeId::new(1);
const START: Timestamp = Timestamp::from_unix_secs(1_800_000_000);

struct Harness {
    dir: tempfile::TempDir,
    clock: Arc<ManualClock>,
    store: Store,
    engine: Engine<crate::testing::SharedLink>,
    bus: broadcast::Receiver<Event>,
    jobs: broadcast::Sender<Arc<Job>>,
    recorder: EventRecorder,
}

fn config() -> EventsConfig {
    EventsConfig {
        tick_every: Duration::from_millis(5),
        flush_every: Duration::from_millis(5),
        started: Some("1.2.3".into()),
        ..EventsConfig::default()
    }
}

impl Harness {
    async fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(ManualClock::new(START));
        let store = Store::open(dir.path().join("test.db"), clock.clone())
            .await
            .unwrap();
        Self::over(dir, clock, store).await
    }

    async fn over(dir: tempfile::TempDir, clock: Arc<ManualClock>, store: Store) -> Self {
        let engine = Engine::new(EngineConfig::default(), clock.clone());
        let bus = engine.subscribe();
        let (jobs, rx) = broadcast::channel(8);
        let recorder = EventRecorder::start(&engine, store.clone(), Some(rx), config()).await;
        Self {
            dir,
            clock,
            store,
            engine,
            bus,
            jobs,
            recorder,
        }
    }

    /// Stops the recorder and starts another over the same store, as a restart does.
    async fn restart(self) -> Self {
        self.recorder.stop().await;
        Self::over(self.dir, self.clock, self.store).await
    }

    fn status(&self, state: NodeState) {
        let status = NodeStatus {
            state,
            since: self.clock.now(),
            reasons: Vec::new(),
        };
        self.engine
            .context()
            .publish(Event::Status { node: NODE, status });
    }

    /// The next events announced as recorded.
    async fn recorded(&mut self) -> Vec<EventRecord> {
        let wait = async {
            loop {
                if let Ok(Event::Recorded { events }) = self.bus.recv().await {
                    return events.to_vec();
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(5), wait)
            .await
            .unwrap()
    }
}

fn names(events: &[EventRecord]) -> Vec<&'static str> {
    events.iter().map(|event| event.what.name()).collect()
}

#[tokio::test]
async fn records_the_start_and_a_node_offline_once_it_lasts_then_back_after_a_restart() {
    let mut h = Harness::start().await;
    assert_eq!(
        h.recorded().await[0].what,
        EventKind::ControllerStarted {
            version: "1.2.3".into()
        }
    );
    h.status(NodeState::Online);
    h.status(NodeState::Offline);
    tokio::time::sleep(Duration::from_millis(30)).await;
    h.clock.advance(Duration::from_secs(31));
    let offline = h.recorded().await;
    assert_eq!(names(&offline), ["node_offline"]);
    assert_eq!(offline[0].at, START);
    let mut h = h.restart().await;
    assert_eq!(names(&h.recorded().await), ["controller_started"]);
    h.clock.advance(Duration::from_secs(29));
    h.status(NodeState::Online);
    let back = h.recorded().await;
    assert_eq!(
        back[0].what,
        EventKind::NodeOnline {
            down_ms: Some(60_000)
        }
    );
    let stored = h
        .store
        .events(EventFilter::default(), None, 10)
        .await
        .unwrap();
    assert_eq!(
        names(&stored),
        [
            "node_online",
            "controller_started",
            "node_offline",
            "controller_started"
        ]
    );
    h.recorder.stop().await;
}

fn radar(id: i64, at: Timestamp) -> LogRecord {
    LogRecord {
        id: LogId::new(id),
        received: at,
        node: Some(NODE),
        host: None,
        via: LogVia::Agent,
        entry: LogEntry {
            seq: None,
            time: at,
            facility: 3,
            severity: Severity::Warning,
            program: Some("hostapd".into()),
            pid: None,
            message: "phy1-ap0: DFS-RADAR-DETECTED freq=5500 ht_enabled=1 chan_offset=0".into(),
        },
        pattern: Some("dfs-radar".into()),
    }
}

#[tokio::test]
async fn log_entries_of_an_event_pattern_fold_into_one_event() {
    let mut h = Harness::start().await;
    h.recorded().await;
    let logged = |records: Vec<LogRecord>| Event::Logged {
        records: records.into(),
    };
    h.engine
        .context()
        .publish(logged(vec![radar(1, START), radar(2, START)]));
    let first = h.recorded().await;
    assert_eq!((names(&first), first[0].count), (vec!["log_pattern"], 2));
    h.clock.advance(Duration::from_secs(60));
    let later = START.saturating_add(Duration::from_secs(60));
    h.engine.context().publish(logged(vec![radar(3, later)]));
    let folded = h.recorded().await;
    assert_eq!(
        (folded[0].id, folded[0].count, folded[0].last_at),
        (first[0].id, 3, later)
    );
    h.recorder.stop().await;
}

#[tokio::test]
async fn an_adoption_from_the_job_runner_is_an_event() {
    let mut h = Harness::start().await;
    h.recorded().await;
    let job = Job {
        id: JobId::new(1),
        kind: JobKind::Adopt,
        target: "root@192.0.2.2:22".into(),
        node: Some(NODE),
        state: JobState::Succeeded,
        revision: None,
        nodes: Vec::new(),
        steps: Vec::new(),
        error: None,
        created_by: None,
        started_at: START,
        finished_at: Some(START),
    };
    h.jobs.send(Arc::new(job)).unwrap();
    let adopted = h.recorded().await;
    assert_eq!(
        (names(&adopted), adopted[0].node),
        (vec!["node_adopted"], Some(NODE))
    );
    h.recorder.stop().await;
}
