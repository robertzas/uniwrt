//! The evaluator over hand-made windows and events, on hand-moved time.

use std::time::Duration;

use uniwrt_core::{EventId, NodeId, RuleId, Timestamp, WindowId};
use uniwrt_model::{
    alerts::{
        AlertCause, AlertRecord, AlertResolution, AlertRule, AlertState, AlertWrite, Comparison,
        EventCondition, MaintenanceWindow, MetricCondition, RuleCondition, RuleSpec,
    },
    events::{Condition, EventCategory, EventKind, EventLevel, EventRecord, NewEvent},
    metric::{Entity, Metric, Sample},
};

use super::{AlertsConfig, Evaluator};

const T0: i64 = 1_800_000_000_000;

fn at(secs: u64) -> Timestamp {
    Timestamp::from_unix_ms(T0).saturating_add(Duration::from_secs(secs))
}

fn rule(id: i64, cooldown_s: u32, when: RuleCondition) -> AlertRule {
    AlertRule {
        id: RuleId::new(id),
        preset: None,
        spec: RuleSpec {
            name: format!("rule {id}"),
            enabled: true,
            level: EventLevel::Warning,
            when,
            cooldown_s,
        },
    }
}

/// CPU over 90 for a minute, clear under 75.
fn cpu(id: i64, cooldown_s: u32) -> AlertRule {
    rule(
        id,
        cooldown_s,
        RuleCondition::Metric(MetricCondition {
            metric: Metric::Cpu,
            entities: "node:*".parse().unwrap(),
            op: Comparison::Above,
            threshold: 90.0,
            clear: Some(75.0),
            for_s: 60,
        }),
    )
}

fn events_rule(id: i64, kinds: &[&str], count: u32, window_s: u32) -> AlertRule {
    rule(
        id,
        0,
        RuleCondition::Event(EventCondition {
            kinds: kinds.iter().map(|&kind| kind.to_owned()).collect(),
            categories: Vec::new(),
            min: None,
            nodes: Vec::new(),
            patterns: Vec::new(),
            count,
            window_s,
        }),
    )
}

/// Nothing changed.
#[track_caller]
fn none(out: &[AlertWrite]) {
    assert_eq!(out, []);
}

fn evaluator(rules: Vec<AlertRule>) -> Evaluator {
    let mut evaluator = Evaluator::new(AlertsConfig::default(), Vec::new(), at(0));
    none(&evaluator.set_rules(rules, at(0)));
    evaluator
}

/// The window starting at `secs` of node 1's CPU.
fn cpu_window(secs: u64, value: f64) -> Sample {
    Sample {
        entity: Entity::Node(NodeId::new(1)),
        metric: Metric::Cpu,
        at: at(secs),
        value,
    }
}

/// Feeds node 1's CPU windows from `from` (every 15 s), returning every write.
fn feed(evaluator: &mut Evaluator, from: u64, values: &[f64]) -> Vec<AlertWrite> {
    let mut out = Vec::new();
    for (i, value) in values.iter().enumerate() {
        out.extend(evaluator.windows(&[cpu_window(from + 15 * i as u64, *value)]));
    }
    out
}

fn written(write: &AlertWrite) -> &AlertState {
    match write {
        AlertWrite::Open { state, .. }
        | AlertWrite::Update { state, .. }
        | AlertWrite::Reopen { state, .. } => state,
    }
}

#[test]
fn a_metric_fires_once_it_lasts_and_clears_below_its_clear_value() {
    let mut evaluator = evaluator(vec![cpu(1, 0)]);
    // Three windows over 90 make 45 s: not yet.
    none(&feed(&mut evaluator, 0, &[95.0, 96.0, 97.0]));
    let out = feed(&mut evaluator, 45, &[93.0]);
    let [AlertWrite::Open { rule, state }] = out.as_slice() else {
        panic!("{out:?}");
    };
    assert_eq!(*rule, RuleId::new(1));
    assert_eq!(state.subject, "node:1");
    assert_eq!(state.node, Some(NodeId::new(1)));
    assert_eq!(state.started_at, at(60));
    assert!(matches!(
        state.cause,
        AlertCause::Metric {
            value: 93.0,
            worst: 93.0,
            ..
        }
    ));
    // Worse updates it; between 75 and 90 it keeps firing quietly.
    let out = feed(&mut evaluator, 60, &[99.0, 80.0, 85.0]);
    assert_eq!(out.len(), 1);
    assert!(matches!(
        written(&out[0]).cause,
        AlertCause::Metric { worst: 99.0, .. }
    ));
    let out = feed(&mut evaluator, 105, &[70.0]);
    let [AlertWrite::Update { state, .. }] = out.as_slice() else {
        panic!("{out:?}");
    };
    assert_eq!(state.resolved_at, Some(at(105)));
    assert_eq!(state.resolution, Some(AlertResolution::Cleared));
}

#[test]
fn a_gap_starts_the_wait_over_and_a_quiet_series_resolves() {
    let mut evaluator = evaluator(vec![cpu(1, 0)]);
    // 45 s over, then two windows missing: the next 45 s are not enough.
    none(&feed(&mut evaluator, 0, &[95.0, 95.0, 95.0]));
    none(&feed(&mut evaluator, 75, &[95.0, 95.0, 95.0]));
    // One window missing is tolerated.
    let out = feed(&mut evaluator, 135, &[95.0]);
    assert!(
        matches!(out.as_slice(), [AlertWrite::Open { .. }]),
        "{out:?}"
    );
    // The series stops: five minutes after its last window, it resolves.
    none(&evaluator.tick(at(150 + 299)));
    let out = evaluator.tick(at(150 + 300));
    assert_eq!(written(&out[0]).resolution, Some(AlertResolution::NoData));
}

#[test]
fn firing_again_within_the_cool_down_reopens() {
    let mut evaluator = evaluator(vec![cpu(1, 600)]);
    feed(&mut evaluator, 0, &[95.0; 4]);
    feed(&mut evaluator, 60, &[50.0]);
    // Back within ten minutes: the same alert, twice now, with its first start.
    let out = feed(&mut evaluator, 120, &[95.0; 4]);
    let [AlertWrite::Reopen { state, .. }] = out.as_slice() else {
        panic!("{out:?}");
    };
    assert_eq!(
        (state.count, state.started_at, state.resolved_at),
        (2, at(60), None)
    );
    feed(&mut evaluator, 180, &[50.0]);
    // After the cool-down, a new alert.
    evaluator.tick(at(180 + 601));
    let out = feed(&mut evaluator, 900, &[95.0; 4]);
    assert!(
        matches!(out.as_slice(), [AlertWrite::Open { .. }]),
        "{out:?}"
    );
}

fn event(id: i64, secs: u64, what: EventKind) -> EventRecord {
    EventRecord::new(
        EventId::new(id),
        NewEvent::new(at(secs), what).node(NodeId::new(3)),
    )
}

fn offline(id: i64, secs: u64, raised: bool) -> EventRecord {
    let what = if raised {
        EventKind::NodeOffline {
            reasons: Vec::new(),
        }
    } else {
        EventKind::NodeOnline { down_ms: None }
    };
    let mut record = event(id, secs, what);
    record.condition = Some(Condition {
        key: "node:3/offline".to_owned(),
        raised,
    });
    record
}

#[test]
fn a_condition_keeps_its_alert_until_it_clears() {
    let mut evaluator = evaluator(vec![events_rule(1, &["node_offline"], 1, 300)]);
    let out = evaluator.events(&[offline(1, 10, true)], at(10));
    let [AlertWrite::Open { state, .. }] = out.as_slice() else {
        panic!("{out:?}");
    };
    assert_eq!(state.subject, "node:3/offline");
    assert_eq!(state.level, EventLevel::Warning);
    // Its window passing changes nothing: the node is still offline.
    none(&evaluator.tick(at(3_600)));
    let out = evaluator.events(&[offline(2, 3_700, false)], at(3_700));
    assert_eq!(written(&out[0]).resolved_at, Some(at(3_700)));
}

#[test]
fn counted_events_fire_with_folded_repeats_and_resolve_with_their_window() {
    let mut evaluator = evaluator(vec![events_rule(1, &["log_pattern"], 5, 600)]);
    let log = |id, secs, count| {
        let mut record = event(
            id,
            secs,
            EventKind::LogPattern {
                pattern: "ssh-bad-password".to_owned(),
                log: uniwrt_core::LogId::new(id),
                host: None,
                program: None,
                text: String::new(),
            },
        );
        record.category = EventCategory::Security;
        record.count = count;
        record.last_at = at(secs + u64::from(count));
        record
    };
    none(&evaluator.events(&[log(1, 0, 1)], at(0)));
    // The same record folds three more repeats in: four.
    none(&evaluator.events(&[log(1, 0, 4)], at(10)));
    // A record seen again with no new repeats counts nothing.
    none(&evaluator.events(&[log(1, 0, 4)], at(11)));
    let out = evaluator.events(&[log(2, 20, 1)], at(21));
    let [AlertWrite::Open { state, .. }] = out.as_slice() else {
        panic!("{out:?}");
    };
    assert_eq!(state.subject, "node:3");
    assert!(matches!(
        state.cause,
        AlertCause::Event {
            matches: 5,
            condition: None,
            ..
        }
    ));
    // Another match updates it.
    let out = evaluator.events(&[log(3, 30, 1)], at(31));
    assert!(matches!(
        written(&out[0]).cause,
        AlertCause::Event { matches: 6, .. }
    ));
    // Once the first four leave the window, fewer than five remain.
    none(&evaluator.tick(at(600)));
    let out = evaluator.tick(at(605));
    assert_eq!(written(&out[0]).resolution, Some(AlertResolution::Cleared));
}

#[test]
fn maintenance_silences_and_lifts() {
    let mut evaluator = evaluator(vec![events_rule(1, &["node_offline"], 1, 300)]);
    evaluator.silences().job(NodeId::new(3), true, at(0));
    let out = evaluator.events(&[offline(1, 10, true)], at(10));
    assert!(written(&out[0]).silenced);
    evaluator.silences().job(NodeId::new(3), false, at(20));
    // Five minutes after the job, the alert still firing is no longer silenced.
    none(&evaluator.tick(at(319)));
    let out = evaluator.tick(at(320));
    assert!(!written(&out[0]).silenced);
    // A window over every node silences the next.
    evaluator.silences().set_windows(vec![MaintenanceWindow {
        id: WindowId::new(1),
        starts_at: at(400),
        ends_at: at(500),
        nodes: Vec::new(),
        note: "moving the AP".to_owned(),
        created_by: None,
    }]);
    let mut other = offline(2, 450, true);
    other.node = Some(NodeId::new(4));
    other.condition = Some(Condition {
        key: "node:4/offline".to_owned(),
        raised: true,
    });
    let out = evaluator.events(&[other], at(450));
    assert!(written(&out[0]).silenced);
}

#[test]
fn rules_changing_resolve_their_alerts_and_disabled_rules_stay_quiet() {
    let mut evaluator = evaluator(vec![cpu(1, 600), events_rule(2, &["node_offline"], 1, 300)]);
    feed(&mut evaluator, 0, &[95.0; 4]);
    evaluator.events(&[offline(1, 10, true)], at(10));
    let mut edited = cpu(1, 600);
    let RuleCondition::Metric(condition) = &mut edited.spec.when else {
        unreachable!()
    };
    condition.threshold = 95.0;
    // Rule 1 changed and rule 2 went: both alerts resolve.
    let out = evaluator.set_rules(vec![edited.clone()], at(100));
    assert_eq!(out.len(), 2);
    assert!(
        out.iter()
            .all(|w| written(w).resolution == Some(AlertResolution::RuleChanged))
    );
    // Firing afresh is a new alert, not a reopening.
    let out = feed(&mut evaluator, 120, &[99.0; 4]);
    assert!(
        matches!(out.as_slice(), [AlertWrite::Open { .. }]),
        "{out:?}"
    );
    edited.spec.enabled = false;
    evaluator.set_rules(vec![edited], at(200));
    none(&feed(&mut evaluator, 300, &[99.0; 8]));
}

#[test]
fn restored_alerts_resume() {
    let restored = |id: i64, rule: i64, subject: &str, condition: Option<&str>| AlertRecord {
        id: uniwrt_core::AlertId::new(id),
        rule: RuleId::new(rule),
        state: AlertState {
            name: "x".to_owned(),
            level: EventLevel::Warning,
            subject: subject.to_owned(),
            node: Some(NodeId::new(3)),
            client: None,
            started_at: at(0),
            last_at: at(0),
            resolved_at: None,
            resolution: None,
            count: 1,
            silenced: false,
            cause: AlertCause::Event {
                event: EventId::new(1),
                kind: "node_offline".to_owned(),
                matches: 1,
                condition: condition.map(str::to_owned),
            },
        },
        acknowledged: None,
    };
    let mut evaluator = Evaluator::new(
        AlertsConfig::default(),
        vec![
            restored(1, 1, "node:3/offline", Some("node:3/offline")),
            restored(2, 2, "node:3", None),
        ],
        at(60),
    );
    evaluator.set_rules(
        vec![
            events_rule(1, &["node_offline"], 1, 300),
            events_rule(2, &["node_rebooted"], 1, 300),
        ],
        at(60),
    );
    // The counted one resolves when its window passes; the condition when it clears.
    none(&evaluator.tick(at(299)));
    let out = evaluator.tick(at(300));
    assert_eq!(out.len(), 1);
    assert_eq!(written(&out[0]).subject, "node:3");
    let out = evaluator.events(&[offline(5, 400, false)], at(400));
    assert_eq!(written(&out[0]).subject, "node:3/offline");
    assert_eq!(written(&out[0]).resolution, Some(AlertResolution::Cleared));
}
