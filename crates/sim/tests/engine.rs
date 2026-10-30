//! The engine over the simulated fleet, on tokio's paused clock: the nodes come online with
//! their facts, ticks and pushes keep them fresh, roams are inferred, logs follow, and a
//! rebooted access point goes offline and comes back.

use std::{sync::Arc, time::Duration};

use tokio::{sync::broadcast, time::Instant};
use uniwrt_core::{NodeId, SystemClock};
use uniwrt_engine::{Engine, EngineConfig, Event};
use uniwrt_model::status::NodeState;
use uniwrt_sim::{SimConfig, SimFleet, SimLink};

async fn start(config: &SimConfig) -> (SimFleet, Engine<SimLink>, broadcast::Receiver<Event>) {
    let clock = Arc::new(SystemClock);
    let fleet = SimFleet::spawn(config, clock.as_ref());
    let engine = Engine::new(EngineConfig::default(), clock);
    let events = engine.subscribe();
    for index in 0..fleet.nodes().len() {
        let link = fleet.link(index).expect("a link per node");
        engine.add(id(index), link, None).await;
    }
    (fleet, engine, events)
}

fn id(index: usize) -> NodeId {
    NodeId::new(i64::try_from(index).expect("a small index") + 1)
}

/// Waits on the paused clock until `ready` holds, at most `limit`.
async fn until(limit: Duration, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + limit;
    while !ready() {
        assert!(Instant::now() < deadline, "not within {limit:?}");
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

fn state(engine: &Engine<SimLink>, index: usize) -> NodeState {
    engine.node(id(index)).expect("watched").status.state
}

#[tokio::test(start_paused = true)]
async fn the_fleet_comes_online_with_its_facts() {
    let (fleet, engine, _) = start(&SimConfig {
        reboot_every: None,
        ..SimConfig::default()
    })
    .await;
    until(Duration::from_secs(10), || {
        (0..4).all(|index| state(&engine, index) == NodeState::Online)
    })
    .await;
    tokio::time::sleep(Duration::from_secs(5)).await;
    let mut stations = 0;
    for (index, node) in fleet.nodes().iter().enumerate() {
        let view = engine.node(id(index)).unwrap();
        let facts = view.facts.expect("facts");
        assert_eq!(facts.board.hostname, node.name);
        assert_eq!(facts.radios.len(), 2);
        assert_eq!(facts.batman.len(), 1);
        stations += facts
            .stations
            .iter()
            .filter(|s| s.iface.ends_with("-ap0"))
            .count();
    }
    assert!((25..=28).contains(&stations), "{stations} Wi-Fi clients");
    engine.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn roams_logs_and_a_reboot_reach_the_bus() {
    let (fleet, engine, mut events) = start(&SimConfig {
        reboot_every: None,
        ..SimConfig::default()
    })
    .await;
    let _logs = engine.follow_logs(id(0)).expect("watched");
    let (mut roams, mut logs, mut node_events) = (0, 0, 0);
    let deadline = Instant::now() + Duration::from_mins(30);
    while Instant::now() < deadline && (roams < 2 || logs < 3) {
        match tokio::time::timeout(Duration::from_secs(60), events.recv()).await {
            Ok(Ok(Event::Roam { roam, .. })) => {
                assert_ne!(roam.from.node, roam.to.node);
                roams += 1;
            }
            Ok(Ok(Event::Log { node, entries, .. })) => {
                assert_eq!(node, id(0));
                assert!(!entries.is_empty());
                logs += 1;
            }
            Ok(Ok(Event::Node { .. })) => node_events += 1,
            Ok(Ok(_) | Err(broadcast::error::RecvError::Lagged(_))) | Err(_) => {}
            Ok(Err(broadcast::error::RecvError::Closed)) => panic!("the bus closed"),
        }
    }
    assert!(roams >= 2 && logs >= 3, "{roams} roams, {logs} log batches");
    assert!(node_events > roams);

    assert!(fleet.reboot(2));
    until(Duration::from_secs(10), || {
        state(&engine, 2) == NodeState::Offline
    })
    .await;
    until(Duration::from_secs(180), || {
        state(&engine, 2) == NodeState::Online
    })
    .await;
    let facts = engine.node(id(2)).unwrap().facts.unwrap();
    assert!(
        facts.system.uptime_s < 200,
        "uptime {} s",
        facts.system.uptime_s
    );
    engine.shutdown().await;
}
