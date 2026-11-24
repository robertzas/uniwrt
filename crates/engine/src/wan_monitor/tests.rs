//! The WAN monitor over a store and an engine watching a scripted gateway whose probe script
//! answers what the test says: a round's samples are stored and kept, an outage is raised once
//! with where it lies and the traceroute's last hop, and cleared with how long it lasted; a
//! disabled monitor probes nothing, and an offline gateway concludes nothing.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use serde_json::{Value, json};
use tokio::sync::broadcast;
use uniwrt_core::{ManualClock, NodeId, Timestamp};
use uniwrt_model::{
    events::{EventKind, NewEvent},
    metric::{Entity, Metric},
    node::NodeRole,
    wan_monitor::{WanFault, WanHealth, WanMonitorSettings},
};
use uniwrt_node::LinkState;
use uniwrt_store::{MetricsRepo as _, NodeRepo as _, SettingsRepo as _, Store};

use super::{OUTAGE_CONDITION, WanMonitor, WanMonitorConfig, WanMonitorHandle, WanMonitorState};
use crate::{
    Engine, EngineConfig, Event, Identity,
    testing::{self, Script, hello, identity, running},
};

const AT: Timestamp = Timestamp::from_unix_secs(1_800_000_000);

/// What the probe script prints: every target answers, or only the first hop, or nothing.
#[derive(Clone, Copy, PartialEq)]
enum Line {
    Up,
    IspDown,
}

fn answer(line: Line) -> Value {
    let rtt = |up: bool| {
        if up {
            json!([5.0, 7.0, 6.0])
        } else {
            json!([])
        }
    };
    let internet = line == Line::Up;
    json!({
        "route": true,
        "first_hop": "10.0.0.1",
        "results": [
            { "key": "first-hop", "sent": 3, "rtt_ms": rtt(true), "error": null },
            { "key": "cloudflare", "sent": 3, "rtt_ms": rtt(internet), "error": null },
            { "key": "google", "sent": 3, "rtt_ms": rtt(internet), "error": null },
            { "key": "quad9", "sent": 3, "rtt_ms": rtt(internet), "error": null },
            { "key": "resolver", "sent": 1, "rtt_ms": rtt(internet), "error": null },
            { "key": "isp-dns@10.0.0.53", "sent": 1, "rtt_ms": rtt(internet), "error": null },
            { "key": "web", "sent": 1, "rtt_ms": rtt(internet), "error": null },
        ],
    })
}

struct Harness {
    _dir: tempfile::TempDir,
    store: Store,
    _engine: Engine<testing::SharedLink>,
    bus: broadcast::Receiver<Event>,
    monitor: Option<WanMonitor>,
    handle: WanMonitorHandle,
    script: Script,
    line: Arc<Mutex<Line>>,
}

impl Harness {
    async fn start(settings: Option<WanMonitorSettings>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(ManualClock::new(AT));
        let store = Store::open(dir.path().join("test.db"), clock.clone())
            .await
            .unwrap();
        if let Some(settings) = settings {
            store
                .put_setting(
                    WanMonitorSettings::KEY,
                    serde_json::to_value(settings).unwrap(),
                )
                .await
                .unwrap();
        }
        let node = store
            .create_node(uniwrt_model::node::NewNode {
                name: "gw".to_owned(),
                mac: identity().mac,
                board_name: "x".to_owned(),
                host: "10.0.0.1".to_owned(),
                port: 22,
                role: NodeRole::Gateway,
            })
            .await
            .unwrap()
            .id;
        let engine = Engine::new(EngineConfig::default(), clock);
        let bus = engine.subscribe();
        let (link, script) = testing::link();
        let wan = Arc::new(Mutex::new(Line::Up));
        let current = Arc::clone(&wan);
        script.answer(move |op, args| match op {
            "exec" if args[0] == "ucode" => {
                let now = *current.lock().unwrap();
                Some(Ok(json!({ "code": 0, "stdout": answer(now).to_string() })))
            }
            "exec" if args[0] == "traceroute" => Some(Ok(json!({
                "code": 0,
                "stdout": " 1  10.0.0.1  1.0 ms\n 2  100.64.1.1  4.0 ms\n 3  *\n",
            }))),
            _ => None,
        });
        let gateway = Identity {
            role: NodeRole::Gateway,
            ..identity()
        };
        engine.add(node, gateway, link, None).await;
        script.set_state(running(hello(true)));
        let config = WanMonitorConfig {
            every: Duration::from_millis(50),
        };
        let monitor = WanMonitor::start(&engine, store.clone(), config);
        let handle = monitor.handle();
        Self {
            _dir: dir,
            store,
            _engine: engine,
            bus,
            monitor: Some(monitor),
            handle,
            script,
            line: wan,
        }
    }

    /// The first state `wanted` takes, within 10 s.
    async fn state(&mut self, wanted: impl Fn(&WanMonitorState) -> bool) -> Arc<WanMonitorState> {
        let wait = async {
            loop {
                let state = self.handle.state();
                if wanted(&state) {
                    return state;
                }
                assert!(self.handle.changed().await, "the monitor stopped");
            }
        };
        tokio::time::timeout(Duration::from_secs(10), wait)
            .await
            .expect("the state within 10 s")
    }

    async fn noted(&mut self) -> NewEvent {
        loop {
            match tokio::time::timeout(Duration::from_secs(10), self.bus.recv()).await {
                Ok(Ok(Event::Noted { event })) => return event,
                Ok(Ok(_) | Err(broadcast::error::RecvError::Lagged(_))) => {}
                other => panic!("no event noted: {other:?}"),
            }
        }
    }

    async fn stop(mut self) {
        if let Some(monitor) = self.monitor.take() {
            monitor.stop().await;
        }
    }
}

#[tokio::test]
async fn a_round_is_stored_as_samples_and_kept() {
    let mut harness = Harness::start(None).await;
    let state = harness.state(|state| state.round.is_some()).await;
    assert_eq!(state.health, Some(WanHealth::Up));
    assert_eq!(state.gateway, Some(NodeId::new(1)));
    assert_eq!(state.since, Some(AT));
    assert_eq!(state.settings, WanMonitorSettings::default());
    let series = harness.store.metric_series(None).await.unwrap();
    let cloudflare = series
        .iter()
        .find(|(entity, _)| *entity == Entity::Probe("cloudflare".to_owned()))
        .map(|(_, metrics)| metrics.clone())
        .expect("the cloudflare series");
    for metric in [Metric::Rtt, Metric::RttMin, Metric::RttMax, Metric::Loss] {
        assert!(cloudflare.contains(&metric), "{metric:?} in {cloudflare:?}");
    }
    assert!(
        series
            .iter()
            .any(|(entity, _)| *entity == Entity::Probe("isp-dns@10.0.0.53".to_owned()))
    );
    harness.stop().await;
}

#[tokio::test]
async fn an_outage_is_raised_once_placed_and_cleared() {
    let mut harness = Harness::start(None).await;
    harness
        .state(|state| state.health == Some(WanHealth::Up))
        .await;
    *harness.line.lock().unwrap() = Line::IspDown;
    let raised = harness.noted().await;
    assert_eq!(
        raised.what,
        EventKind::WanOutage {
            fault: WanFault::Isp,
            last_hop: Some("100.64.1.1".to_owned()),
        }
    );
    let condition = raised.condition.clone().unwrap();
    assert_eq!(
        (condition.key.as_str(), condition.raised),
        (OUTAGE_CONDITION, true)
    );
    let state = harness
        .state(|state| state.health == Some(WanHealth::Down(WanFault::Isp)))
        .await;
    assert_eq!(state.last_hop, Some("100.64.1.1".parse().unwrap()));
    // Later rounds of the same outage raise nothing more: the next event clears it.
    tokio::time::sleep(Duration::from_millis(200)).await;
    *harness.line.lock().unwrap() = Line::Up;
    let cleared = harness.noted().await;
    assert!(matches!(
        cleared.what,
        EventKind::WanRestored {
            fault: WanFault::Isp,
            down_ms: Some(_),
        }
    ));
    assert!(!cleared.condition.unwrap().raised);
    harness.stop().await;
}

#[tokio::test]
async fn a_disabled_monitor_probes_nothing() {
    let settings = WanMonitorSettings {
        enabled: false,
        ..WanMonitorSettings::default()
    };
    let mut harness = Harness::start(Some(settings)).await;
    let state = harness.state(|state| !state.settings.enabled).await;
    assert_eq!(state.round, None);
    harness
        .script
        .no_call_of("exec", Duration::from_millis(300))
        .await;
    harness.stop().await;
}

#[tokio::test]
async fn an_offline_gateway_concludes_nothing() {
    let mut harness = Harness::start(None).await;
    harness
        .state(|state| state.health == Some(WanHealth::Up))
        .await;
    harness
        .script
        .set_state(LinkState::Connecting { attempt: 2 });
    let state = harness.state(|state| state.error.is_some()).await;
    assert_eq!(state.health, Some(WanHealth::Up));
    assert_eq!(state.round, None);
    harness.stop().await;
}
