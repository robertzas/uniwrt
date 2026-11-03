//! Alerts over a real database: rules and presets, the engine's writes by rule and subject,
//! filters and pages, what the engine resumes, acknowledgements, retention, and maintenance
//! windows.

use std::time::Duration;

use uniwrt_core::{AlertId, EventId, NodeId, RuleId, Timestamp, UserId};
use uniwrt_model::{
    alerts::{
        Acknowledgement, AlertCause, AlertFilter, AlertResolution, AlertState, AlertWrite,
        EventCondition, NewWindow, RuleCondition, RuleSpec,
    },
    events::EventLevel,
};

use crate::{
    AlertRepo as _,
    testing::{START, open},
};

fn at(secs: u64) -> Timestamp {
    START.saturating_add(Duration::from_secs(secs))
}

fn spec(name: &str) -> RuleSpec {
    RuleSpec {
        name: name.to_owned(),
        enabled: true,
        level: EventLevel::Critical,
        when: RuleCondition::Event(EventCondition {
            kinds: vec!["node_offline".to_owned()],
            categories: Vec::new(),
            min: None,
            nodes: Vec::new(),
            patterns: Vec::new(),
            count: 1,
            window_s: 300,
        }),
        cooldown_s: 600,
    }
}

fn state(subject: &str, node: i64, started: u64) -> AlertState {
    AlertState {
        name: "Device offline".to_owned(),
        level: EventLevel::Critical,
        subject: subject.to_owned(),
        node: Some(NodeId::new(node)),
        client: None,
        started_at: at(started),
        last_at: at(started),
        resolved_at: None,
        resolution: None,
        count: 1,
        silenced: false,
        cause: AlertCause::Event {
            event: EventId::new(1),
            kind: "node_offline".to_owned(),
            matches: 1,
            condition: None,
        },
    }
}

fn resolved(mut state: AlertState, secs: u64) -> AlertState {
    state.resolved_at = Some(at(secs));
    state.resolution = Some(AlertResolution::Cleared);
    state
}

#[tokio::test]
async fn rules_seed_once_and_presets_stay() {
    let (_dir, store, _clock) = open().await;
    let presets = vec![("node-offline".to_owned(), spec("Device offline"))];
    assert_eq!(store.seed_alert_rules(presets.clone()).await.unwrap(), 1);
    assert_eq!(store.seed_alert_rules(presets).await.unwrap(), 0);
    let mine = store.create_alert_rule(spec("Mine")).await.unwrap();
    let rules = store.alert_rules().await.unwrap();
    assert_eq!(rules.len(), 2);
    let preset = &rules[0];
    assert_eq!(preset.preset.as_deref(), Some("node-offline"));
    // Edited, but never deleted.
    let mut edited = spec("Renamed");
    edited.enabled = false;
    let updated = store
        .update_alert_rule(preset.id, edited.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.spec, edited);
    assert!(!store.delete_alert_rule(preset.id).await.unwrap());
    assert!(store.delete_alert_rule(mine.id).await.unwrap());
    assert_eq!(store.alert_rule(mine.id).await.unwrap(), None);
    assert_eq!(
        store.update_alert_rule(mine.id, spec("x")).await.unwrap(),
        None
    );
}

#[tokio::test]
async fn writes_change_the_latest_alert_of_a_rule_and_subject() {
    let (_dir, store, _clock) = open().await;
    let rule = RuleId::new(1);
    let first = state("node:3", 3, 0);
    let opened = store
        .record_alerts(vec![AlertWrite::Open {
            rule,
            state: first.clone(),
        }])
        .await
        .unwrap();
    let id = opened[0].id;
    // Another subject of the same rule stays apart.
    store
        .record_alerts(vec![AlertWrite::Open {
            rule,
            state: state("node:4", 4, 5),
        }])
        .await
        .unwrap();
    let ack = Acknowledgement {
        by: UserId::new(1),
        username: "admin".to_owned(),
        at: at(10),
    };
    let acked = store.acknowledge_alert(id, ack.clone()).await.unwrap();
    assert_eq!(acked.unwrap().acknowledged, Some(ack.clone()));
    let stored = store
        .record_alerts(vec![AlertWrite::Update {
            rule,
            state: resolved(first.clone(), 20),
        }])
        .await
        .unwrap();
    assert_eq!(stored[0].id, id);
    assert_eq!(
        stored[0].acknowledged,
        Some(ack.clone()),
        "an update keeps it"
    );
    // Resolved alerts are not acknowledged.
    assert_eq!(store.acknowledge_alert(id, ack).await.unwrap(), None);
    let mut again = first;
    again.count = 2;
    let reopened = store
        .record_alerts(vec![AlertWrite::Reopen { rule, state: again }])
        .await
        .unwrap();
    assert_eq!((reopened[0].id, reopened[0].state.count), (id, 2));
    assert_eq!(reopened[0].acknowledged, None, "a reopen drops it");
    // A write to a pair with no alert is skipped.
    let none = store
        .record_alerts(vec![AlertWrite::Update {
            rule: RuleId::new(9),
            state: state("node:3", 3, 0),
        }])
        .await
        .unwrap();
    assert_eq!(none, []);
}

#[tokio::test]
async fn filters_pages_and_resumes() {
    let (_dir, store, _clock) = open().await;
    let write = |rule: i64, state: AlertState| AlertWrite::Open {
        rule: RuleId::new(rule),
        state,
    };
    store
        .record_alerts(vec![
            write(1, resolved(state("node:1", 1, 0), 10)),
            write(1, resolved(state("node:1", 1, 100), 110)),
            write(2, state("node:2", 2, 200)),
            write(3, resolved(state("site", 1, 300), 900)),
        ])
        .await
        .unwrap();
    let all = |filter| store.alerts(filter, None, 100);
    assert_eq!(all(AlertFilter::default()).await.unwrap().len(), 4);
    let active = all(AlertFilter {
        active: true,
        ..AlertFilter::default()
    })
    .await
    .unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].state.subject, "node:2");
    let node1 = all(AlertFilter {
        nodes: vec![NodeId::new(1)],
        rules: vec![RuleId::new(1)],
        ..AlertFilter::default()
    })
    .await
    .unwrap();
    assert_eq!(node1.len(), 2);
    // Pages, newest first.
    let page = store.alerts(AlertFilter::default(), None, 2).await.unwrap();
    assert_eq!(page[0].state.started_at, at(300));
    let last = &page[1];
    let next = store
        .alerts(
            AlertFilter::default(),
            Some((last.state.started_at, last.id)),
            10,
        )
        .await
        .unwrap();
    assert_eq!(next.len(), 2);
    assert_eq!(next[0].state.started_at, at(100));
    // Resuming: the active one, and the latest of each pair resolved since 500 s.
    let resume = store.alerts_to_resume(at(500)).await.unwrap();
    let subjects: Vec<&str> = resume.iter().map(|a| a.state.subject.as_str()).collect();
    assert_eq!(subjects, ["node:2", "site"]);
    assert_eq!(
        store.alert(AlertId::new(1)).await.unwrap().unwrap().id,
        AlertId::new(1)
    );
    // Retention deletes resolved ones only.
    assert_eq!(store.prune_alerts(at(200)).await.unwrap(), 2);
    assert_eq!(all(AlertFilter::default()).await.unwrap().len(), 2);
}

#[tokio::test]
async fn maintenance_windows() {
    let (_dir, store, _clock) = open().await;
    let window = |start: u64, end: u64, nodes: Vec<NodeId>| NewWindow {
        starts_at: at(start),
        ends_at: at(end),
        nodes,
        note: "firmware".to_owned(),
    };
    let first = store
        .create_maintenance_window(window(0, 100, Vec::new()), Some(UserId::new(1)))
        .await
        .unwrap();
    store
        .create_maintenance_window(window(50, 300, vec![NodeId::new(2)]), None)
        .await
        .unwrap();
    let on = store.maintenance_windows(at(100)).await.unwrap();
    assert_eq!(on.len(), 1);
    assert_eq!(on[0].nodes, [NodeId::new(2)]);
    assert_eq!(store.maintenance_windows(at(0)).await.unwrap()[0], first);
    assert!(store.delete_maintenance_window(first.id).await.unwrap());
    assert!(!store.delete_maintenance_window(first.id).await.unwrap());
}
