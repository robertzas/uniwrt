//! The scanner over a store and an engine watching the lab gateway's capture (radio0 on 2.4 GHz
//! through `phy1-ap0`, radio1 on 5 GHz through `phy2-ap0`), its scans answered by hand: the
//! background schedule scans each radio and the report shows what they heard; with scanning off,
//! only an admin's scans run, and a radio whose driver cannot scan waits a day.

use std::{collections::BTreeMap, sync::Arc, time::Duration};

use serde_json::{Value, json};
use tokio::sync::broadcast;
use uniwrt_core::{ManualClock, NodeId, Timestamp};
use uniwrt_model::{
    node::NodeMode,
    rf::{RfSettings, ScanState, ScanTrigger},
    site::{
        SiteConfig,
        node::{ManagedPackage, NodeConfig},
    },
    wifi::Band,
};
use uniwrt_node::Error;
use uniwrt_rf::{NeighborKind, RfView, plan::Reason};
use uniwrt_store::{NodeRepo as _, RfRepo as _, Store};

use super::{RfConfig, RfScanner};
use crate::{
    Engine, EngineConfig, Event,
    testing::{self, Script, SharedLink, hello, identity, running},
};

const NOW: Timestamp = Timestamp::from_unix_secs(1_800_000_000);
/// An SSID element naming the network `cafe`, in base64.
const CAFE: &str = "AARjYWZl";

struct Harness {
    _dir: tempfile::TempDir,
    store: Store,
    engine: Engine<SharedLink>,
    bus: broadcast::Receiver<Event>,
    scanner: RfScanner,
    node: NodeId,
    script: Script,
}

fn config() -> RfConfig {
    RfConfig {
        tick_every: Duration::from_millis(10),
        first_delay: Duration::ZERO,
        stagger: Duration::ZERO,
        gap: Duration::ZERO,
        rebuild_after: Duration::ZERO,
        ..RfConfig::default()
    }
}

/// A scan through `ifname` hearing `cafe` on its band.
fn scanned(ifname: &str) -> Value {
    let frequency = if ifname.starts_with("phy1") {
        2437
    } else {
        5180
    };
    json!({
        "started_ms": 1_000, "ended_ms": 3_500,
        "bss": [{
            "bssid": "0c:00:00:00:00:01", "frequency": frequency, "signal_mbm": -6000,
            "capability": 1, "seen_ms_ago": 100, "ies": CAFE
        }],
        "survey_before": [], "survey_after": [{"frequency": frequency, "noise": -95, "time": 120, "busy": 30}]
    })
}

impl Harness {
    async fn start(settings: Option<RfSettings>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(ManualClock::new(NOW));
        let store = Store::open(dir.path().join("test.db"), clock.clone())
            .await
            .unwrap();
        let node = store
            .create_node(uniwrt_model::node::NewNode {
                name: "gw".to_owned(),
                mac: identity().mac,
                board_name: "x".to_owned(),
                host: "10.0.0.1".to_owned(),
                port: 22,
                role: uniwrt_model::node::NodeRole::Gateway,
            })
            .await
            .unwrap()
            .id;
        if let Some(settings) = settings {
            use uniwrt_store::SettingsRepo as _;
            let value = serde_json::to_value(settings).unwrap();
            store.put_setting(RfSettings::KEY, value).await.unwrap();
        }
        let engine = Engine::new(EngineConfig::default(), clock);
        let bus = engine.subscribe();
        let scanner = RfScanner::start(&engine, store.clone(), config()).await;
        let (link, script) = testing::link();
        engine.add(node, identity(), link, None).await;
        Self {
            _dir: dir,
            store,
            engine,
            bus,
            scanner,
            node,
            script,
        }
    }

    fn up(&self) {
        self.script.set_state(running(hello(true)));
    }

    /// The first view that `want` accepts.
    async fn view(&mut self, want: impl Fn(&RfView) -> bool) -> Arc<RfView> {
        let search = async {
            loop {
                if let Event::Rf { view } = self.bus.recv().await.unwrap()
                    && want(&view)
                {
                    return view;
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(20), search)
            .await
            .expect("the view within 20 s")
    }
}

fn state(view: &RfView, radio: &str) -> Option<ScanState> {
    view.radios
        .iter()
        .find(|r| r.radio == radio)
        .map(|r| r.state)
}

#[tokio::test]
async fn the_schedule_scans_each_radio_and_the_report_shows_what_they_heard() {
    let mut h = Harness::start(None).await;
    h.script.answer(|op, args| {
        (op == "scan").then(|| Ok(scanned(args["ifname"].as_str().unwrap_or_default())))
    });
    h.up();
    let call = h.script.call_of("scan").await;
    assert_eq!(call.args["ifname"], "phy1-ap0");
    assert!(
        call.args["freqs"]
            .as_array()
            .is_some_and(|f| f.contains(&json!(2412)))
    );
    let view = h
        .view(|v| v.radios.len() == 2 && v.radios.iter().all(|r| r.last.is_some()))
        .await;
    for radio in &view.radios {
        assert_eq!(radio.state, ScanState::Scheduled, "{radio:?}");
        assert_eq!(radio.last.as_ref().unwrap().trigger, ScanTrigger::Scheduled);
        // Due again in four hours.
        assert_eq!(
            radio.next_at,
            Some(
                radio
                    .last
                    .as_ref()
                    .unwrap()
                    .at
                    .saturating_add(Duration::from_hours(4))
            )
        );
    }
    let band = view
        .report
        .bands
        .iter()
        .find(|b| b.band == Band::G5)
        .unwrap();
    let cafe = &band.networks[0];
    assert_eq!(cafe.bss.ssid.as_deref(), Some("cafe"));
    assert_eq!(cafe.kind, NeighborKind::Foreign);
    assert_eq!(cafe.heard_by[0].node, h.node);
    assert_eq!(band.radios[0].on_channel.len(), 1, "cafe is on 36");
    let stored = h.store.latest_scans(NOW).await.unwrap();
    assert_eq!(stored.len(), 2);
    assert_eq!(h.scanner.handle().view().radios.len(), 2);

    // The planner sees both radios: observed, they keep what they run; managed, they're its.
    let handle = h.scanner.handle();
    let mut site = SiteConfig::defaults(None);
    let plan = handle.plan(site.clone(), BTreeMap::new()).await.unwrap();
    let radios: Vec<_> = plan.bands.iter().flat_map(|b| &b.radios).collect();
    assert_eq!(radios.len(), 2);
    assert!(
        radios
            .iter()
            .all(|r| !r.changed && r.reasons[0] == Reason::Observed)
    );
    let mut config = NodeConfig::new(h.node);
    config.managed.insert(ManagedPackage::Wireless);
    site.nodes.push(config);
    let modes = BTreeMap::from([(h.node, NodeMode::Managed)]);
    let plan = handle.plan(site, modes).await.unwrap();
    let radios: Vec<_> = plan.bands.iter().flat_map(|b| &b.radios).collect();
    assert_eq!(radios.len(), 2);
    assert!(
        radios
            .iter()
            .all(|r| !r.reasons.contains(&Reason::Observed))
    );
    h.scanner.stop().await;
    h.engine.shutdown().await;
}

#[tokio::test]
async fn with_scanning_off_only_an_admin_s_scans_run_and_unsupported_ones_wait_a_day() {
    let off = RfSettings {
        scanning: false,
        interval_h: 4,
        ..RfSettings::default()
    };
    let mut h = Harness::start(Some(off)).await;
    h.script.answer(|op, args| {
        (op == "scan").then(|| match args["ifname"].as_str() {
            Some("phy2-ap0") => Err(Error::Remote {
                op: "scan".to_owned(),
                code: "unsupported".to_owned(),
                msg: "the radio cannot scan: Operation not supported".to_owned(),
            }),
            _ => Ok(scanned("phy1-ap0")),
        })
    });
    h.up();
    let view = h
        .view(|v| v.radios.len() == 2 && v.radios.iter().all(|r| r.state == ScanState::Off))
        .await;
    assert!(!view.settings.scanning);
    let scan = tokio::time::timeout(Duration::from_millis(300), h.script.call_of("scan")).await;
    assert!(scan.is_err(), "no background scan: {scan:?}");

    let handle = h.scanner.handle();
    let queued = handle.scan(Some(h.node), None).await.unwrap();
    assert_eq!(queued.len(), 2);
    let view = h
        .view(|v| {
            state(v, "radio1") == Some(ScanState::Unsupported)
                && v.radios.iter().any(|r| r.last.is_some())
        })
        .await;
    let radio0 = view.radios.iter().find(|r| r.radio == "radio0").unwrap();
    assert_eq!(radio0.last.as_ref().unwrap().trigger, ScanTrigger::Manual);
    let radio1 = view.radios.iter().find(|r| r.radio == "radio1").unwrap();
    assert!(radio1.error.as_deref().unwrap().contains("not supported"));
    assert!(radio1.next_at.unwrap() >= NOW.saturating_add(Duration::from_hours(24)));
    assert!(handle.scan(Some(NodeId::new(99)), None).await.is_err());

    // Turning scanning on keeps the radio that cannot scan waiting.
    handle
        .set_settings(RfSettings {
            scanning: true,
            interval_h: 6,
            ..RfSettings::default()
        })
        .await
        .unwrap();
    let view = handle.view();
    assert_eq!(view.settings.interval_h, 6);
    assert_eq!(state(&view, "radio1"), Some(ScanState::Unsupported));
    h.scanner.stop().await;
    h.engine.shutdown().await;
}
