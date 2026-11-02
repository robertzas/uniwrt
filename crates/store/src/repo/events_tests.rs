//! Events over a real database: recording and folding, filters and pages, the conditions
//! still raised, and retention by age and count.

use std::time::Duration;

use uniwrt_core::{EventId, NodeId, Timestamp};
use uniwrt_model::events::{
    EventCategory, EventFilter, EventKind, EventLevel, EventWrite, NewEvent,
};

use crate::{
    EventRepo as _, EventsPrune,
    testing::{START, open},
};

const GW: NodeId = NodeId::new(1);
const AP: NodeId = NodeId::new(2);

fn at(secs: u64) -> Timestamp {
    START.saturating_add(Duration::from_secs(secs))
}

fn offline(node: NodeId, secs: u64) -> NewEvent {
    NewEvent::new(
        at(secs),
        EventKind::NodeOffline {
            reasons: Vec::new(),
        },
    )
    .node(node)
    .condition(format!("node:{node}/offline"), true)
}

fn online(node: NodeId, secs: u64) -> NewEvent {
    NewEvent::new(
        at(secs),
        EventKind::NodeOnline {
            down_ms: Some(1_000),
        },
    )
    .node(node)
    .condition(format!("node:{node}/offline"), false)
}

fn joined(secs: u64) -> NewEvent {
    NewEvent::new(at(secs), EventKind::ClientLeft { connected_ms: None })
        .node(AP)
        .client("f0:18:98:00:00:01".parse().unwrap())
}

fn inserts(events: Vec<NewEvent>) -> Vec<EventWrite> {
    events.into_iter().map(EventWrite::Insert).collect()
}

#[tokio::test]
async fn records_reads_back_and_folds() {
    let (_dir, store, _clock) = open().await;
    let recorded = store
        .record_events(inserts(vec![offline(GW, 1), joined(2)]))
        .await
        .unwrap();
    let first = recorded[0].clone().expect("an insert is stored");
    assert_eq!(
        first.what,
        EventKind::NodeOffline {
            reasons: Vec::new()
        }
    );
    assert_eq!(first.condition.as_ref().map(|c| c.raised), Some(true));
    let folded = store
        .record_events(vec![
            EventWrite::Fold {
                id: first.id,
                last_at: at(9),
                count: 2,
            },
            EventWrite::Fold {
                id: EventId::new(99),
                last_at: at(9),
                count: 1,
            },
        ])
        .await
        .unwrap();
    let again = folded[0].clone().expect("the repeat is stored");
    assert_eq!((again.count, again.last_at, again.at), (3, at(9), at(1)));
    assert_eq!(folded[1], None, "no such event");
    let all = store
        .events(EventFilter::default(), None, 10)
        .await
        .unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].what.name(), "client_left", "newest first");
    assert_eq!(all[1], again);
}

#[tokio::test]
async fn filters_and_pages() {
    let (_dir, store, _clock) = open().await;
    store
        .record_events(inserts(vec![
            offline(GW, 1),
            joined(2),
            offline(AP, 3),
            joined(4),
        ]))
        .await
        .unwrap();
    let read = |filter: EventFilter| {
        let store = store.clone();
        async move {
            store
                .events(filter, None, 10)
                .await
                .unwrap()
                .into_iter()
                .map(|event| event.at)
                .collect::<Vec<_>>()
        }
    };
    let critical = EventFilter {
        min: Some(EventLevel::Warning),
        ..EventFilter::default()
    };
    assert_eq!(read(critical).await, [at(3), at(1)]);
    let clients = EventFilter {
        categories: vec![EventCategory::Client],
        ..EventFilter::default()
    };
    assert_eq!(read(clients).await, [at(4), at(2)]);
    let ap = EventFilter {
        nodes: vec![AP],
        ..EventFilter::default()
    };
    assert_eq!(read(ap).await, [at(4), at(3), at(2)]);
    let phone = EventFilter {
        clients: vec!["f0:18:98:00:00:01".parse().unwrap()],
        from: Some(at(3)),
        ..EventFilter::default()
    };
    assert_eq!(read(phone).await, [at(4)]);
    let page = store.events(EventFilter::default(), None, 2).await.unwrap();
    let last = page.last().unwrap();
    let next = store
        .events(EventFilter::default(), Some((last.at, last.id)), 2)
        .await
        .unwrap();
    assert_eq!(
        next.iter().map(|e| e.at).collect::<Vec<_>>(),
        [at(2), at(1)]
    );
}

#[tokio::test]
async fn knows_the_conditions_still_raised() {
    let (_dir, store, _clock) = open().await;
    store
        .record_events(inserts(vec![
            offline(GW, 1),
            offline(AP, 2),
            online(GW, 3),
            joined(4),
        ]))
        .await
        .unwrap();
    assert_eq!(
        store.raised_conditions().await.unwrap(),
        [("node:2/offline".to_owned(), at(2))]
    );
}

#[tokio::test]
async fn prunes_by_age_then_by_count() {
    let (_dir, store, _clock) = open().await;
    store
        .record_events(inserts((0..10).map(joined).collect()))
        .await
        .unwrap();
    let prune = |before, max_events| EventsPrune {
        before: at(before),
        max_events,
    };
    assert_eq!(store.prune_events(prune(3, 100)).await.unwrap(), 3);
    assert_eq!(store.prune_events(prune(3, 100)).await.unwrap(), 0);
    assert_eq!(store.prune_events(prune(0, 4)).await.unwrap(), 3);
    let left = store
        .events(EventFilter::default(), None, 100)
        .await
        .unwrap();
    assert_eq!(
        left.iter().map(|e| e.at).collect::<Vec<_>>(),
        [at(9), at(8), at(7), at(6)]
    );
}
