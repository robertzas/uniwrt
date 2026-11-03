//! The alert engine over a store and an engine without nodes, fed by hand on a manual clock:
//! the built-in rules stored once, an event alert raised and cleared, a metric rule of the
//! user's firing from windows, acknowledgements, a rule disabled, silencing while a job runs, and
//! a restart that resumes what fires.

use std::{sync::Arc, time::Duration};

use tokio::sync::broadcast;
use uniwrt_core::{Clock as _, EventId, JobId, ManualClock, NodeId, Timestamp, UserId};
use uniwrt_model::{
    alerts::{
        Acknowledgement, AlertFilter, AlertRecord, AlertResolution, Comparison, MetricCondition,
        RuleCondition, RuleSpec,
    },
    events::{Condition, EventKind, EventLevel, EventRecord, NewEvent},
    job::{Job, JobKind, JobState},
    metric::{Entity, Metric, Sample},
};
use uniwrt_store::{AlertRepo as _, Store};

use super::{AlertEngine, AlertsConfig};
use crate::{Engine, EngineConfig, Event};

const NODE: NodeId = NodeId::new(3);
const START: Timestamp = Timestamp::from_unix_secs(1_800_000_000);

struct Harness {
    dir: tempfile::TempDir,
    clock: Arc<ManualClock>,
    store: Store,
    engine: Engine<crate::testing::SharedLink>,
    bus: broadcast::Receiver<Event>,
    jobs: broadcast::Sender<Arc<Job>>,
    alerts: AlertEngine,
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
        let config = AlertsConfig {
            tick_every: Duration::from_millis(5),
            ..AlertsConfig::default()
        };
        let alerts = AlertEngine::start(&engine, store.clone(), Some(rx), config).await;
        Self {
            dir,
            clock,
            store,
            engine,
            bus,
            jobs,
            alerts,
        }
    }

    async fn restart(self) -> Self {
        self.alerts.stop().await;
        Self::over(self.dir, self.clock, self.store).await
    }

    fn publish(&self, event: Event) {
        self.engine.context().publish(event);
    }

    fn offline(&self, id: i64, raised: bool) {
        let what = if raised {
            EventKind::NodeOffline {
                reasons: Vec::new(),
            }
        } else {
            EventKind::NodeOnline { down_ms: None }
        };
        let mut record = EventRecord::new(
            EventId::new(id),
            NewEvent::new(self.clock.now(), what).node(NODE),
        );
        record.condition = Some(Condition {
            key: "node:3/offline".to_owned(),
            raised,
        });
        self.publish(Event::Recorded {
            events: Arc::from([record]),
        });
    }

    /// The next alerts announced.
    async fn announced(&mut self) -> Vec<AlertRecord> {
        let wait = async {
            loop {
                if let Ok(Event::Alerts { alerts }) = self.bus.recv().await {
                    return alerts.to_vec();
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(5), wait)
            .await
            .unwrap()
    }

    async fn rule_id(&self, preset: &str) -> uniwrt_core::RuleId {
        let rules = self.store.alert_rules().await.unwrap();
        rules
            .iter()
            .find(|rule| rule.preset.as_deref() == Some(preset))
            .unwrap()
            .id
    }
}

#[tokio::test]
async fn an_event_alert_fires_is_acknowledged_and_clears() {
    let mut h = Harness::start().await;
    let presets = h.store.alert_rules().await.unwrap();
    assert!(presets.len() >= 15, "the built-in rules are stored");
    h.offline(1, true);
    let opened = h.announced().await;
    assert_eq!(opened.len(), 1);
    let alert = &opened[0];
    assert_eq!(alert.rule, h.rule_id("node-offline").await);
    assert_eq!(alert.state.level, EventLevel::Critical);
    assert_eq!(alert.state.node, Some(NODE));
    let ack = Acknowledgement {
        by: UserId::new(1),
        username: "admin".to_owned(),
        at: START,
    };
    let acked = h.alerts.handle().acknowledge(alert.id, ack.clone()).await;
    assert_eq!(acked.unwrap().unwrap().acknowledged, Some(ack.clone()));
    assert_eq!(h.announced().await[0].acknowledged, Some(ack));
    h.clock.advance(Duration::from_secs(60));
    h.offline(2, false);
    let cleared = h.announced().await;
    assert_eq!(cleared[0].id, alert.id);
    assert_eq!(cleared[0].state.resolution, Some(AlertResolution::Cleared));
    let active = h
        .store
        .alerts(
            AlertFilter {
                active: true,
                ..AlertFilter::default()
            },
            None,
            10,
        )
        .await
        .unwrap();
    assert_eq!(active, []);
}

#[tokio::test]
async fn a_users_metric_rule_fires_from_windows_and_resolves_when_disabled() {
    let mut h = Harness::start().await;
    let mut spec = RuleSpec {
        name: "Busy gateway".to_owned(),
        enabled: true,
        level: EventLevel::Notice,
        when: RuleCondition::Metric(MetricCondition {
            metric: Metric::Load,
            entities: "node:*".parse().unwrap(),
            op: Comparison::Above,
            threshold: 2.0,
            clear: None,
            for_s: 0,
        }),
        cooldown_s: 0,
    };
    let rule = h.store.create_alert_rule(spec.clone()).await.unwrap();
    h.alerts.handle().reload().await.unwrap();
    h.publish(Event::Sampled {
        samples: Arc::from([Sample {
            entity: Entity::Node(NODE),
            metric: Metric::Load,
            at: START,
            value: 3.5,
        }]),
    });
    let opened = h.announced().await;
    assert_eq!(
        (opened[0].rule, opened[0].state.subject.as_str()),
        (rule.id, "node:3")
    );
    spec.enabled = false;
    h.store.update_alert_rule(rule.id, spec).await.unwrap();
    h.alerts.handle().reload().await.unwrap();
    let resolved = h.announced().await;
    assert_eq!(
        resolved[0].state.resolution,
        Some(AlertResolution::RuleChanged)
    );
}

#[tokio::test]
async fn alerts_during_a_job_are_silenced_and_resume_after_a_restart() {
    let mut h = Harness::start().await;
    let job = Job {
        id: JobId::new(1),
        kind: JobKind::Adopt,
        target: "root@10.0.0.3:22".to_owned(),
        node: Some(NODE),
        state: JobState::Running,
        steps: Vec::new(),
        error: None,
        created_by: None,
        started_at: START,
        finished_at: None,
    };
    h.jobs.send(Arc::new(job)).unwrap();
    // The job reaches the engine before the event does: both go through one task, but on
    // separate channels.
    tokio::time::sleep(Duration::from_millis(20)).await;
    h.offline(1, true);
    let opened = h.announced().await;
    assert!(opened[0].state.silenced);
    let mut h = h.restart().await;
    // Resumed: the clear resolves the same alert.
    h.offline(2, false);
    let cleared = h.announced().await;
    assert_eq!(cleared[0].id, opened[0].id);
    assert!(cleared[0].state.resolved_at.is_some());
}
