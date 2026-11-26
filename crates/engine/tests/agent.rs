//! The engine watching real OpenWrt (TESTING.md, Agent): an actor over an SSH link to the
//! agent, in both loop modes. Ignored by default: `just test-agent` boots an OpenWrt rootfs
//! container per release with procd and runs these against it, as it runs the node crate's
//! `tests/agent.rs` (whose helpers this shares), and measures the agent's memory for the
//! performance budgets (D-149).

#![allow(clippy::unwrap_used)]

#[path = "../../node/tests/openwrt/mod.rs"]
mod openwrt;

use std::{env, sync::Arc, time::Duration};

use openwrt::{adopt, link, run};
use tokio::sync::broadcast;
use uniwrt_core::MacAddr;
use uniwrt_core::{NodeId, SystemClock};
use uniwrt_engine::{Engine, EngineConfig, Event, Identity};
use uniwrt_model::{node::NodeRole, status::NodeState};
use uniwrt_node::{
    AgentLaunch, NodeLink as _,
    proto::ops::{Exec, Set},
};

const NODE: NodeId = NodeId::new(1);
const IDENTITY: Identity = Identity {
    role: NodeRole::Gateway,
    mac: MacAddr::new([2, 0, 0, 0, 0, 1]),
};

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
    engine.add(NODE, IDENTITY, link, None).await;

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

/// Prints the resident memory of the agent above this shell, in KiB, as `peak current`: the
/// farthest `ucode … main.uc` among its ancestors, as the event loop runs each command in a
/// forked task (other tests' agents run beside it).
const AGENT_RSS: &str = r#"
p=$$ agent=
while [ "$p" -gt 1 ]; do
  p=$(awk '/^PPid:/ { print $2 }' "/proc/$p/status")
  if tr '\0' ' ' <"/proc/$p/cmdline" | grep -q 'main\.uc'; then agent=$p; fi
done
[ -n "$agent" ] || exit 1
awk '/^VmHWM:/ { peak = $2 } /^VmRSS:/ { now = $2 } END { print peak, now }' "/proc/$agent/status"
"#;

/// The agent's memory as the engine runs it (D-149): every set at its base interval, the
/// pushes subscribed and the log followed where the event loop runs, for 40 s (each 15 s and
/// 30 s set collected again), then its peak resident memory. Written in bytes to the file
/// `UNIWRT_PERF_AGENT_RSS` names, if any, which `scripts/perf.sh agent-rss` checks.
#[tokio::test]
#[ignore = "needs an OpenWrt container booted with procd: just test-agent"]
async fn the_agent_s_memory_under_the_engine() {
    let release = env::var("UNIWRT_NODE_RELEASE").expect("UNIWRT_NODE_RELEASE");
    let (_session, node) = adopt().await;
    let (link, hello) = link(&node, AgentLaunch::Bundle).await;
    let engine = Engine::new(EngineConfig::default(), Arc::new(SystemClock));
    let mut bus = engine.subscribe();
    engine.add(NODE, IDENTITY, link, None).await;
    next(&mut bus, Duration::from_secs(20), |event| match event {
        Event::Facts { .. } => Some(()),
        _ => None,
    })
    .await;
    let follow = hello.caps.events.then(|| engine.follow_logs(NODE).unwrap());
    tokio::time::sleep(Duration::from_secs(40)).await;

    let link = engine.link(NODE).unwrap();
    let read = link
        .request(&Exec::new(["sh", "-c", AGENT_RSS]), Duration::from_secs(20))
        .await
        .unwrap();
    assert!(read.success(), "{read:?}");
    let kib: Vec<u64> = read
        .stdout
        .split_whitespace()
        .map(|n| n.parse().unwrap())
        .collect();
    let [peak, now] = kib[..] else {
        panic!("{read:?}")
    };
    println!("the agent on {release}: peak RSS {peak} KiB, now {now} KiB");
    if let Ok(path) = env::var("UNIWRT_PERF_AGENT_RSS") {
        std::fs::write(path, (peak * 1024).to_string()).unwrap();
    }
    drop(follow);
    engine.shutdown().await;
    link.shutdown().await;
}
