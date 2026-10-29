//! The engine watching real OpenWrt (TESTING.md, Agent): an actor over an SSH link to the
//! agent, in both loop modes. Ignored by default: `just test-agent` boots an OpenWrt rootfs
//! container per release with procd and runs these against it, as it runs the node crate's
//! `tests/agent.rs` (whose helpers this shares).

#![allow(clippy::unwrap_used)]

#[path = "../../node/tests/openwrt/mod.rs"]
mod openwrt;

use std::{env, sync::Arc, time::Duration};

use openwrt::{adopt, link, run};
use tokio::sync::broadcast;
use uniwrt_core::{NodeId, SystemClock};
use uniwrt_engine::{Engine, EngineConfig, Event};
use uniwrt_model::status::NodeState;
use uniwrt_node::{AgentLaunch, proto::ops::Set};

const NODE: NodeId = NodeId::new(1);

/// Waits up to `limit` for the first event that `want` picks.
async fn next<T>(
    bus: &mut broadcast::Receiver<Event>,
    limit: Duration,
    mut want: impl FnMut(Event) -> Option<T>,
) -> T {
    tokio::time::timeout(limit, async {
        loop {
            if let Some(found) = want(bus.recv().await.unwrap()) {
                return found;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("no such event within {limit:?}"))
}

#[tokio::test]
#[ignore = "needs an OpenWrt container booted with procd: just test-agent"]
async fn the_engine_watches_a_node_and_speeds_up_while_focused() {
    let release = env::var("UNIWRT_NODE_RELEASE").expect("UNIWRT_NODE_RELEASE");
    let (session, node) = adopt().await;
    let (link, hello) = link(&node, AgentLaunch::Bundle).await;
    let engine = Engine::new(EngineConfig::default(), Arc::new(SystemClock));
    let mut bus = engine.subscribe();
    engine.add(NODE, link, None).await;

    let facts = next(&mut bus, Duration::from_secs(20), |event| match event {
        Event::Facts { facts, .. } => Some(facts),
        _ => None,
    })
    .await;
    assert_eq!(facts.board.release.version, release);
    assert!(!facts.interfaces.is_empty() && facts.system.memory.total_bytes > 0);
    let status = engine.node(NODE).unwrap().status;
    assert_eq!((status.state, status.reasons), (NodeState::Online, vec![]));

    // Focused, the system set comes every 2 s: ticked by the event loop, or polled.
    let focus = engine.focus(NODE).unwrap();
    let uptime = facts.system.uptime_s;
    let faster = next(&mut bus, Duration::from_secs(8), |event| match event {
        Event::Facts { facts, changed, .. } if changed.contains(&Set::System) => {
            (facts.system.uptime_s > uptime).then_some(facts)
        }
        _ => None,
    })
    .await;
    assert!(faster.system.uptime_s <= uptime + 6);
    drop(focus);

    if hello.caps.events {
        let follow = engine.follow_logs(NODE).unwrap();
        tokio::time::sleep(Duration::from_secs(2)).await;
        let text = format!("engine test {}", std::process::id());
        run(&session, &["logger", "-t", "uniwrt-test", &text]).await;
        next(&mut bus, Duration::from_secs(10), |event| match event {
            Event::Log { entries, .. } => entries.iter().any(|e| e.message == text).then_some(()),
            _ => None,
        })
        .await;
        drop(follow);
    }
    let link = engine.link(NODE).unwrap();
    engine.shutdown().await;
    link.shutdown().await;
}
