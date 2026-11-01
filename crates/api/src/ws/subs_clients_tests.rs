//! The clients' topics over a real book fed with the canonical gateway's facts: snapshots,
//! views merged by MAC address between flushes, a client's samples and sessions.

use std::sync::Arc;

use serde_json::Value;
use uniwrt_clients::Changes;
use uniwrt_core::Timestamp;
use uniwrt_engine::Event;
use uniwrt_model::{facts::NodeFacts, node::NodeRole, status::NodeState};
use uniwrt_topo::Member;

use super::{FakeFleet, harness};

/// The gateway's facts collected at `secs`, built and taken into the fleet's book.
fn collect(fleet: &FakeFleet, secs: i64) -> Changes {
    let mut facts: NodeFacts =
        serde_json::from_str(include_str!("../../../model/fixtures/facts/gateway.json")).unwrap();
    facts.collected_at = Timestamp::from_unix_secs(secs);
    let members = [Member {
        id: uniwrt_core::NodeId::new(1),
        role: NodeRole::Gateway,
        mac: "02:00:00:00:00:01".parse().unwrap(),
        state: NodeState::Online,
        facts: Some(&facts),
    }];
    let topology = uniwrt_topo::build(&members, None);
    fleet.book().update(&topology, &members)
}

#[tokio::test]
async fn the_clients_topics_merge_views_and_queue_samples_and_sessions() {
    let mut h = harness().await;
    let first = collect(&h.fleet, 1_000);
    let wifi = first
        .clients
        .iter()
        .find(|client| client.experience.is_some())
        .expect("a Wi-Fi client in the fixture")
        .mac;
    let topic = format!("client:{wifi}");
    let snapshots = h.set(&["clients", &topic]).await;
    assert_eq!(snapshots[0]["type"], "client_snapshot");
    assert_eq!(snapshots[0]["client"]["client"]["mac"], wifi.to_string());
    assert_eq!(
        snapshots[0]["client"]["samples"].as_array().unwrap().len(),
        1
    );
    let sessions = snapshots[0]["client"]["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 1, "the open one, which the store lacks");
    assert_eq!(snapshots[1]["type"], "clients_snapshot");
    assert_eq!(
        snapshots[1]["clients"].as_array().unwrap().len(),
        first.clients.len()
    );
    assert_eq!(h.flush(), Vec::<Value>::new());

    // Two collects between flushes: each client once, as it is last.
    let second = collect(&h.fleet, 1_010);
    let third = collect(&h.fleet, 1_020);
    for changes in [second, third] {
        h.apply(Event::Clients {
            changes: Arc::new(changes),
        })
        .await;
    }
    let deltas = h.flush();
    let client = deltas
        .iter()
        .find(|delta| delta["type"] == "client_delta")
        .unwrap();
    assert_eq!(client["topic"], topic.as_str());
    assert_eq!(client["samples"].as_array().unwrap().len(), 2);
    assert_eq!(client["client"]["last_seen"], 1_020_000);
    let clients = deltas
        .iter()
        .find(|delta| delta["type"] == "clients_delta")
        .unwrap();
    let macs: Vec<&str> = clients["clients"]
        .as_array()
        .unwrap()
        .iter()
        .map(|client| client["mac"].as_str().unwrap())
        .collect();
    let mut unique = macs.clone();
    unique.dedup();
    assert_eq!(macs, unique);
    assert!(macs.contains(&wifi.to_string().as_str()));

    // Forgotten.
    h.apply(Event::Clients {
        changes: Arc::new(Changes {
            removed: vec![wifi],
            ..Changes::default()
        }),
    })
    .await;
    let deltas = h.flush();
    assert_eq!(deltas[0]["removed"], true);
    assert_eq!(deltas[1]["removed"][0], wifi.to_string());
    let refused = h.set(&["client:02:00:00:00:00:fe"]).await;
    assert_eq!(refused[0]["type"], "error");
}
