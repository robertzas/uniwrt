//! Actors over a scripted link, on tokio's paused clock: what they ask the agent, and what
//! they announce.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use serde_json::{Value, json};
use tokio::sync::broadcast;
use uniwrt_core::{ManualClock, NodeId, Timestamp};
use uniwrt_model::{
    event::{Attachment, NodeEvent, StationChange},
    status::{NodeState, NodeStatus, StatusReason},
};
use uniwrt_node::{AgentState, Error, LinkState, proto::ops::Set};

use crate::{
    Engine, EngineConfig, Event,
    testing::{self, Script, SharedLink, collected, hello, identity, running},
};

const NODE: NodeId = NodeId::new(1);
const NOW: Timestamp = Timestamp::from_unix_secs(1_793_200_000);
const BASE: [(u64, &[&str]); 4] = [
    (
        15_000,
        &["devices", "interfaces", "radios", "stations", "system"],
    ),
    (30_000, &["batman", "neighbors"]),
    (60_000, &["leases"]),
    (3_600_000, &["capabilities", "phys"]),
];

struct Fleet {
    engine: Engine<SharedLink>,
    bus: broadcast::Receiver<Event>,
}

impl Fleet {
    fn new() -> Self {
        let clock = Arc::new(ManualClock::new(NOW));
        let engine = Engine::new(EngineConfig::default(), clock);
        let bus = engine.subscribe();
        Self { engine, bus }
    }

    /// Adds a node whose agent runs, with an event loop or without.
    async fn node(&self, node: NodeId, events: bool) -> Script {
        let (link, script) = testing::link();
        self.engine.add(node, identity(), link, None).await;
        script.set_state(running(hello(events)));
        script
    }

    /// The next event that `want` picks, skipping others.
    async fn next<T>(&mut self, want: impl Fn(Event) -> Option<T>) -> T {
        let search = async {
            loop {
                if let Some(found) = want(self.bus.recv().await.unwrap()) {
                    return found;
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(120), search)
            .await
            .expect("the event within two minutes")
    }

    async fn status(&mut self, node: NodeId) -> NodeStatus {
        self.next(|event| match event {
            Event::Status { node: of, status } if of == node => Some(status),
            _ => None,
        })
        .await
    }

    async fn status_of(&mut self, node: NodeId, state: NodeState) -> NodeStatus {
        loop {
            let status = self.status(node).await;
            if status.state == state {
                return status;
            }
        }
    }

    async fn facts(&mut self, node: NodeId) -> Vec<Set> {
        self.next(|event| match event {
            Event::Facts {
                node: of, changed, ..
            } if of == node => Some(changed),
            _ => None,
        })
        .await
    }
}

/// The intervals of the next `count` `set_interval` requests.
async fn schedule(script: &mut Script, count: usize) -> Vec<(u64, Vec<String>)> {
    let mut groups = Vec::new();
    for _ in 0..count {
        let call = script.call_of("set_interval").await;
        let sets = serde_json::from_value(call.args["sets"].clone()).unwrap();
        groups.push((call.args["interval_ms"].as_u64().unwrap(), sets));
    }
    groups
}

fn groups(groups: &[(u64, &[&str])]) -> Vec<(u64, Vec<String>)> {
    groups
        .iter()
        .map(|(interval, sets)| (*interval, sets.iter().map(|&set| set.to_owned()).collect()))
        .collect()
}

fn sets(call: &testing::Call) -> Vec<String> {
    serde_json::from_value(call.args["sets"].clone()).unwrap()
}

/// Waits until the agent was asked for every push of a new start.
async fn set_up(script: &mut Script) {
    assert_eq!(script.call().await.op, "subscribe");
    assert_eq!(script.call().await.op, "collect");
    assert_eq!(schedule(script, 4).await, groups(&BASE));
}

#[tokio::test(start_paused = true)]
async fn a_new_agent_is_subscribed_collected_and_scheduled() {
    let mut fleet = Fleet::new();
    let mut script = fleet.node(NODE, true).await;
    let subscribe = script.call().await;
    assert_eq!(
        (subscribe.op.as_str(), subscribe.args),
        (
            "subscribe",
            json!({"topics": ["dhcp", "hostapd", "netifd"]})
        )
    );
    let collect = script.call().await;
    assert_eq!((collect.op.as_str(), sets(&collect).len()), ("collect", 11));
    assert_eq!(schedule(&mut script, 4).await, groups(&BASE));
    assert_eq!(fleet.status_of(NODE, NodeState::Online).await.reasons, []);
    assert_eq!(fleet.facts(NODE).await.len(), 11);
    let view = fleet.engine.node(NODE).unwrap();
    let facts = view.facts.unwrap();
    assert_eq!(facts.collected_at, NOW);
    assert!(!facts.radios.is_empty() && !facts.phys.is_empty());
    assert_eq!(view.status.state, NodeState::Online);
    // Nobody follows the log, and the agent ticks by itself (until its ticks are late).
    script.no_call(Duration::from_secs(40)).await;
}

#[tokio::test(start_paused = true)]
async fn ticks_update_the_facts_and_only_changes_are_announced() {
    let mut fleet = Fleet::new();
    let mut script = fleet.node(NODE, true).await;
    set_up(&mut script).await;
    fleet.facts(NODE).await;
    script.push_tick(&collected(&[Set::System, Set::Leases]));
    let mut tick = collected(&[Set::System]);
    let uptime = &mut tick["sets"]["system"]["info"]["uptime"];
    *uptime = json!(uptime.as_u64().unwrap() + 15);
    script.push_tick(&tick);
    assert_eq!(fleet.facts(NODE).await, [Set::System]);
}

#[tokio::test(start_paused = true)]
async fn station_events_are_announced_refresh_their_set_and_reveal_roams() {
    let (a, b) = (NodeId::new(1), NodeId::new(2));
    let mut fleet = Fleet::new();
    let mut script_a = fleet.node(a, true).await;
    let mut script_b = fleet.node(b, true).await;
    set_up(&mut script_a).await;
    set_up(&mut script_b).await;
    let authorized =
        |alg: &str| json!({"address": "02:00:00:00:0a:00", "ifname": "phy1-ap0", "auth-alg": alg});
    script_a.push_event("hostapd.sta-authorized", &authorized("sae"));
    let (reported_at, event) = fleet
        .next(|event| match event {
            Event::Node {
                node,
                reported_at,
                event,
                ..
            } if node == a => Some((reported_at, event)),
            _ => None,
        })
        .await;
    assert_eq!(
        reported_at,
        Some(Timestamp::from_unix_ms(1_793_245_539_282))
    );
    let NodeEvent::Station(station) = event else {
        panic!("a station event: {event:?}");
    };
    assert_eq!(station.change, StationChange::Authorized);
    script_b.push_event("hostapd.sta-authorized", &authorized("ft"));
    let roam = fleet
        .next(|event| match event {
            Event::Roam { roam, .. } => Some(roam),
            _ => None,
        })
        .await;
    let attachment = |node| Attachment {
        node,
        iface: "phy1-ap0".to_owned(),
    };
    assert_eq!(
        (roam.from, roam.to, roam.auth_alg.as_deref()),
        (attachment(a), attachment(b), Some("ft"))
    );
    // Half a second later, the stations of a are collected again, and nothing else.
    let refresh = script_a.call().await;
    assert_eq!(
        (refresh.op.as_str(), sets(&refresh)),
        ("collect", vec!["stations".to_owned()])
    );
}

#[tokio::test(start_paused = true)]
async fn focus_speeds_ticks_up_and_letting_go_slows_them_down() {
    let fleet = Fleet::new();
    let mut script = fleet.node(NODE, true).await;
    set_up(&mut script).await;
    let focus = fleet.engine.focus(NODE).unwrap();
    let focused = groups(&[
        (2_000, &["devices", "radios", "stations", "system"]),
        (10_000, &["batman"]),
        (15_000, &["interfaces"]),
        (30_000, &["neighbors"]),
        (60_000, &["leases"]),
        (3_600_000, &["capabilities", "phys"]),
    ]);
    assert_eq!(schedule(&mut script, 6).await, focused);
    // A second viewer changes nothing.
    let second = fleet.engine.focus(NODE).unwrap();
    drop(focus);
    script.no_call(Duration::from_secs(5)).await;
    drop(second);
    assert_eq!(schedule(&mut script, 4).await, groups(&BASE));
    assert!(fleet.engine.focus(NodeId::new(9)).is_none());
}

#[tokio::test(start_paused = true)]
async fn an_agent_without_an_event_loop_is_polled() {
    let fleet = Fleet::new();
    let mut script = fleet.node(NODE, false).await;
    let first = script.call().await;
    assert_eq!((first.op.as_str(), sets(&first).len()), ("collect", 11));
    let poll = script.call().await;
    assert_eq!(sets(&poll), BASE[0].1);
    let _focus = fleet.engine.focus(NODE).unwrap();
    let fast = script.call().await;
    assert_eq!(sets(&fast), ["devices", "radios", "stations", "system"]);
    let next = script.call().await;
    assert_eq!(sets(&next), ["devices", "radios", "stations", "system"]);
}

#[tokio::test(start_paused = true)]
async fn link_states_become_statuses() {
    let mut fleet = Fleet::new();
    let (link, script) = testing::link();
    fleet.engine.add(NODE, identity(), link, None).await;
    let connecting = fleet.status(NODE).await;
    assert_eq!(
        (connecting.state, connecting.reasons),
        (
            NodeState::Offline,
            vec![StatusReason::Connecting { attempt: 1 }]
        )
    );
    let retry_at = NOW.saturating_add(Duration::from_secs(5));
    script.set_state(LinkState::Up {
        since: NOW,
        agent: AgentState::Failed {
            error: "ucode is missing".to_owned(),
            retry_at,
        },
    });
    let degraded = fleet.status(NODE).await;
    assert_eq!(
        (degraded.state, degraded.reasons),
        (
            NodeState::Degraded,
            vec![StatusReason::AgentFailed {
                error: "ucode is missing".to_owned(),
                retry_at
            }]
        )
    );
    script.set_state(running(hello(true)));
    fleet.status_of(NODE, NodeState::Online).await;
    script.set_state(LinkState::Down {
        error: "connection refused".to_owned(),
        retry_at,
    });
    let down = fleet.status(NODE).await;
    assert_eq!(down.state, NodeState::Offline);
    assert_eq!(fleet.engine.node(NODE).unwrap().status, down);
}

#[tokio::test(start_paused = true)]
async fn late_telemetry_degrades_the_node_and_asks_the_agent_again() {
    let mut fleet = Fleet::new();
    let mut script = fleet.node(NODE, true).await;
    set_up(&mut script).await;
    fleet.status_of(NODE, NodeState::Online).await;
    let late = fleet.status(NODE).await;
    assert_eq!(
        (late.state, late.reasons),
        (
            NodeState::Degraded,
            vec![StatusReason::TelemetryStale { last: Some(NOW) }]
        )
    );
    assert_eq!(script.call().await.op, "subscribe");
    assert_eq!(schedule(&mut script, 4).await, groups(&BASE));
    script.push_tick(&collected(&[Set::System]));
    fleet.status_of(NODE, NodeState::Online).await;
}

#[tokio::test(start_paused = true)]
async fn failing_sets_degrade_the_node_until_they_are_collected() {
    let mut fleet = Fleet::new();
    let (link, mut script) = testing::link();
    script.answer(|op, args| {
        (op == "collect").then(|| {
            let sets: Vec<Set> = serde_json::from_value(args["sets"].clone()).unwrap();
            let mut answer = collected(&sets);
            let batman = answer["sets"].as_object_mut().unwrap().remove("batman");
            if batman.is_some() {
                answer["errors"] =
                    json!({"batman": {"code": "failed", "msg": "batctl is not installed"}});
            }
            Ok(answer)
        })
    });
    fleet.engine.add(NODE, identity(), link, None).await;
    script.set_state(running(hello(true)));
    let degraded = fleet.status_of(NODE, NodeState::Degraded).await;
    assert_eq!(
        degraded.reasons,
        [StatusReason::CollectFailed {
            set: "batman".to_owned(),
            error: "failed on the node: failed: batctl is not installed".to_owned()
        }]
    );
    set_up(&mut script).await;
    script.push_tick(&collected(&[Set::Batman]));
    fleet.status_of(NODE, NodeState::Online).await;
}

#[tokio::test(start_paused = true)]
async fn an_agent_that_refuses_ticks_is_polled() {
    let mut fleet = Fleet::new();
    let (link, mut script) = testing::link();
    script.answer(|op, _| {
        (op == "set_interval").then(|| {
            Err(Error::Remote {
                op: op.to_owned(),
                code: "failed".to_owned(),
                msg: "no timers".to_owned(),
            })
        })
    });
    fleet.engine.add(NODE, identity(), link, None).await;
    script.set_state(running(hello(true)));
    let degraded = fleet.status_of(NODE, NodeState::Degraded).await;
    assert_eq!(
        degraded.reasons,
        [StatusReason::RequestFailed {
            op: "set_interval".to_owned(),
            error: "agent op set_interval failed: failed: no timers".to_owned()
        }]
    );
    assert_eq!(sets(&script.call_of("collect").await).len(), 11);
    assert_eq!(script.call().await.op, "set_interval");
    let poll = script.call().await;
    assert_eq!(
        (poll.op.as_str(), sets(&poll)),
        ("collect", groups(&BASE)[0].1.clone())
    );
}

fn log_batch(ids: &[u64]) -> Value {
    let entries: Vec<_> = ids
        .iter()
        .map(|id| {
            json!({"msg": format!("dnsmasq[2092]: line {id}"), "id": id, "priority": 30, "source": 1,
                "time": 1_793_245_536_000_u64 + id})
        })
        .collect();
    json!({"entries": entries, "dropped": 0})
}

#[tokio::test(start_paused = true)]
async fn logs_are_followed_while_wanted_and_resume_without_repeats() {
    let mut fleet = Fleet::new();
    let mut script = fleet.node(NODE, true).await;
    set_up(&mut script).await;
    let follow = fleet.engine.follow_logs(NODE).unwrap();
    let call = script.call().await;
    assert_eq!(
        (call.op.as_str(), call.args),
        (
            "log_follow",
            json!({"mode": "poll", "backfill": 200, "interval_ms": 1000})
        )
    );
    let logs = |event| match event {
        Event::Log { entries, .. } => Some(entries.iter().map(|e| e.seq).collect::<Vec<_>>()),
        _ => None,
    };
    script.push_event("log", &log_batch(&[1, 2]));
    assert_eq!(fleet.next(logs).await, [Some(1), Some(2)]);
    // The agent restarts: the follow starts again and its backfill repeats what was announced.
    script.set_state(LinkState::Up {
        since: NOW,
        agent: AgentState::Starting,
    });
    fleet.status_of(NODE, NodeState::Degraded).await;
    script.set_state(running(hello(true)));
    set_up(&mut script).await;
    assert_eq!(script.call().await.op, "log_follow");
    script.push_event("log", &log_batch(&[1, 2, 3]));
    assert_eq!(fleet.next(logs).await, [Some(3)]);
    drop(follow);
    let stop = script.call().await;
    assert_eq!(
        (stop.op.as_str(), &stop.args["mode"]),
        ("log_follow", &json!("off"))
    );
}

#[tokio::test(start_paused = true)]
async fn removed_nodes_stop_and_are_announced() {
    let mut fleet = Fleet::new();
    let mut script = fleet.node(NODE, true).await;
    set_up(&mut script).await;
    assert!(fleet.engine.remove(NODE).await);
    fleet
        .next(|event| matches!(event, Event::Removed { node } if node == NODE).then_some(()))
        .await;
    assert!(fleet.engine.node(NODE).is_none() && fleet.engine.link(NODE).is_none());
    assert!(!fleet.engine.remove(NODE).await);
    // Its actor is gone: a tick changes nothing.
    script.push_tick(&collected(&[Set::System]));
    script.no_call(Duration::from_secs(60)).await;
    assert_eq!(fleet.engine.all(), []);
}

#[tokio::test(start_paused = true)]
async fn an_actor_that_panics_starts_again() {
    let mut fleet = Fleet::new();
    let (link, mut script) = testing::link();
    let panicked = Arc::new(AtomicBool::new(false));
    let first = Arc::clone(&panicked);
    script.answer(move |op, _| {
        assert!(
            op != "collect" || first.swap(true, Ordering::Relaxed),
            "a bug"
        );
        None
    });
    fleet.engine.add(NODE, identity(), link, None).await;
    script.set_state(running(hello(true)));
    assert_eq!(script.call_of("collect").await.op, "collect");
    // The new actor finds the agent running and starts over.
    set_up(&mut script).await;
    assert_eq!(fleet.facts(NODE).await.len(), 11);
    assert!(panicked.load(Ordering::Relaxed));
}

#[tokio::test(start_paused = true)]
async fn a_replaced_link_keeps_the_facts_and_the_interests() {
    let mut fleet = Fleet::new();
    let mut script = fleet.node(NODE, true).await;
    set_up(&mut script).await;
    fleet.facts(NODE).await;
    let _focus = fleet.engine.focus(NODE).unwrap();
    assert_eq!(schedule(&mut script, 6).await[0].0, 2_000);
    // An admin re-trusted the node: a new link replaces the old one.
    let (link, mut replacement) = testing::link();
    fleet.engine.add(NODE, identity(), link, None).await;
    assert!(fleet.engine.node(NODE).unwrap().facts.is_some());
    replacement.set_state(running(hello(true)));
    assert_eq!(replacement.call().await.op, "subscribe");
    assert_eq!(replacement.call().await.op, "collect");
    assert_eq!(schedule(&mut replacement, 6).await[0].0, 2_000);
}
