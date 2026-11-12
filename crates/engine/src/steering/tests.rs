//! Client steering over a store and an engine watching two nodes (the lab gateway's capture:
//! client `02:00:00:00:04:00`, with 802.11v, on node 1's 2.4 GHz access point `phy1-ap0` of
//! `uniwrt-lab`, which `phy2-ap0` also broadcasts on 5 GHz), the agent's `btm` and `ubus` ops
//! answered by hand: a strong 2.4 GHz client is asked up a band once the profile's interval
//! has passed, and again only an interval later; a fading one is suggested the access points
//! usteer hears it at clearly better, best first; Off, Conservative's missing roam triggers
//! and a client without 802.11v ask nothing.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};

use serde_json::{Value, json};
use tokio::sync::watch;
use uniwrt_core::{MacAddr, ManualClock, Timestamp};
use uniwrt_model::{
    node::{NewNode, NodeRole},
    site::{
        SiteConfig,
        node::{ManagedPackage, NodeConfig},
        revision::{NewRevision, RevisionKind},
        steering::SteeringProfile,
        wireless::{NodeScope, Roaming, Wlan, WlanSecurity},
    },
    wifi::Band,
};
use uniwrt_node::proto::ops::{OwnReport, candidate};
use uniwrt_store::{NodeRepo as _, RevisionRepo as _, Store};

use super::{ClientSteering, OwnReports, SteeringConfig};
use crate::{
    Engine, EngineConfig, Identity,
    testing::{self, Call, Script, hello, running},
};

const NOW: Timestamp = Timestamp::from_unix_secs(1_800_000_000);
const QUIET: Duration = Duration::from_millis(300);
const CLIENT: &str = "02:00:00:00:04:00";
/// A report's fixed part: BSSID, BSSID information, operating class, channel, PHY type.
const NR: &str = "020000000100ef090000732409";

/// What node 1's client looks like, and what usteer answers about it.
struct Setup {
    profile: SteeringProfile,
    signal: i64,
    btm: bool,
    usteer: Value,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            profile: SteeringProfile::Balanced,
            signal: -20,
            btm: true,
            usteer: json!({"nodes": {}}),
        }
    }
}

struct Harness {
    _dir: tempfile::TempDir,
    clock: Arc<ManualClock>,
    engine: Engine<testing::SharedLink>,
    steering: Option<ClientSteering>,
    scripts: Vec<Script>,
    _own: watch::Sender<Arc<OwnReports>>,
}

/// Node `k`'s capture: node 2 at other addresses; node 1's client as `setup` says.
fn capture(k: u8, setup: &Setup, sets: &Value) -> Value {
    let sets: Vec<uniwrt_node::proto::ops::Set> = serde_json::from_value(sets.clone()).unwrap();
    let mut collected = testing::collected(&sets);
    if k == 2 {
        let text = collected
            .to_string()
            .replace("172.17.0.2", "172.17.0.3")
            .replace("10.99.0.1", "10.99.0.2");
        collected = serde_json::from_str(&text).unwrap();
    }
    let stations = &mut collected["sets"]["stations"];
    if k == 1 && stations.is_object() {
        let info = &mut stations["stations"]["phy1-ap0"][0]["info"];
        info["signal"] = json!(setup.signal);
        info["signal_avg"] = json!(setup.signal);
        if !setup.btm {
            // Extended capabilities without BSS transition (bit 19).
            stations["clients"]["phy1-ap0"][CLIENT]["extended_capabilities"] =
                json!([4, 0, 0x42, 2, 1, 64, 0, 64, 0, 1]);
        }
    } else if stations.is_object() {
        stations["stations"]["phy1-ap0"] = json!([]);
        stations["clients"]["phy1-ap0"] = json!({});
    }
    collected
}

fn own(ifname: &str, k: u8) -> OwnReport {
    serde_json::from_value(
        json!({"ifname": ifname, "bssid": format!("02:00:00:00:0{k}:01"),
        "ssid": "uniwrt-lab", "nr": NR}),
    )
    .unwrap()
}

impl Harness {
    async fn start(setup: Setup) -> Self {
        let setup = Arc::new(setup);
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(ManualClock::new(NOW));
        let store = Store::open(dir.path().join("test.db"), clock.clone())
            .await
            .unwrap();
        let mut config = SiteConfig::defaults(None);
        config.steering.profile = setup.profile;
        config.wlans = vec![wlan("uniwrt-lab")];
        let mut nodes = Vec::new();
        for k in 1..=2u8 {
            let node = NewNode {
                name: format!("ap{k}"),
                mac: MacAddr::new([2, 0, 0, 0, 0, k]),
                board_name: "x".to_owned(),
                host: format!("10.0.0.{k}"),
                port: 22,
                role: NodeRole::Ap,
            };
            let node = store.create_node(node).await.unwrap().id;
            let mut node_config = NodeConfig::new(node);
            node_config.managed =
                BTreeSet::from([ManagedPackage::Wireless, ManagedPackage::Usteer]);
            config.nodes.push(node_config);
            nodes.push(node);
        }
        let revision = NewRevision {
            base: None,
            kind: RevisionKind::Defaults,
            restored_from: None,
            author_id: None,
            author: None,
            message: "test".to_owned(),
            config,
        };
        store.commit_revision(revision, Vec::new()).await.unwrap();
        let engine = Engine::new(EngineConfig::default(), clock.clone());
        let reports: OwnReports = BTreeMap::from([
            (nodes[0], vec![own("phy1-ap0", 1), own("phy2-ap0", 1)]),
            (nodes[1], vec![own("phy1-ap0", 2), own("phy2-ap0", 2)]),
        ]);
        let (own_tx, own_rx) = watch::channel(Arc::new(reports));
        let config = SteeringConfig {
            tick_every: Duration::from_millis(50),
            ..SteeringConfig::default()
        };
        let steering = ClientSteering::start(&engine, store, None, own_rx, config);
        let mut scripts = Vec::new();
        for (k, &node) in (1..=2u8).zip(&nodes) {
            let (link, script) = testing::link();
            let setup = Arc::clone(&setup);
            script.answer(move |op, args| match op {
                "collect" => Some(Ok(capture(k, &setup, &args["sets"]))),
                "ubus" if args["path"] == "usteer" => Some(Ok(setup.usteer.clone())),
                "btm" => Some(Ok(
                    json!({"ifname": "phy1-ap0", "token": 1, "timer": 0, "mbo": true}),
                )),
                _ => None,
            });
            let identity = Identity {
                role: NodeRole::Ap,
                mac: MacAddr::new([2, 0, 0, 0, 0, k]),
            };
            engine.add(node, identity, link, None).await;
            script.set_state(running(hello(true)));
            scripts.push(script);
        }
        Self {
            _dir: dir,
            clock,
            engine,
            steering: Some(steering),
            scripts,
            _own: own_tx,
        }
    }

    /// The next request of `op` to node `i` within `wait`, if any.
    async fn call(&mut self, i: usize, op: &str, wait: Duration) -> Option<Call> {
        tokio::time::timeout(wait, self.scripts[i].call_of(op))
            .await
            .ok()
    }

    async fn stop(mut self) {
        if let Some(steering) = self.steering.take() {
            steering.stop().await;
        }
        self.engine.shutdown().await;
    }
}

/// A WPA2 WLAN named `ssid` on 2.4 and 5 GHz everywhere, steered.
fn wlan(ssid: &str) -> Wlan {
    Wlan {
        id: ssid.to_owned(),
        ssid: ssid.to_owned(),
        enabled: true,
        network: "lan".to_owned(),
        security: WlanSecurity::Wpa2,
        passphrase: None,
        bands: vec![Band::G2, Band::G5],
        nodes: NodeScope::All,
        hidden: false,
        isolation: false,
        max_clients: None,
        schedule: None,
        roaming: Roaming::default(),
    }
}

#[tokio::test]
async fn a_strong_2ghz_client_is_asked_up_a_band_once_per_interval() {
    let mut h = Harness::start(Setup::default()).await;
    // Heard strong, but not yet for Balanced's minute.
    assert!(h.call(0, "btm", QUIET).await.is_none());
    h.clock.advance(Duration::from_secs(61));
    let call = h.call(0, "btm", Duration::from_secs(5)).await.unwrap();
    assert_eq!(
        call.args,
        json!({"addr": CLIENT, "neighbors": [candidate(NR, 255).unwrap()], "reason": 9,
            "imminent": false, "kick_ms": 0, "reassoc_s": 0})
    );
    // Not again within the interval; again after it.
    assert!(h.call(0, "btm", QUIET).await.is_none());
    h.clock.advance(Duration::from_secs(61));
    assert!(h.call(0, "btm", Duration::from_secs(5)).await.is_some());
    // The other node has no client to steer.
    assert!(h.call(1, "btm", QUIET).await.is_none());
    h.stop().await;
}

#[tokio::test]
async fn a_fading_client_is_suggested_where_usteer_hears_it_clearly_better() {
    let usteer = json!({"2ghz": true, "5ghz": true, "nodes": {
        // Where it is.
        "hostapd.phy1-ap0": {"connected": true, "signal": -80},
        // Below the trigger (-75), though 8 dB better.
        "hostapd.phy2-ap0": {"connected": false, "signal": -72 - 5},
        // Node 2's, best first; and a host no node has.
        "172.17.0.3#hostapd.phy1-ap0": {"connected": false, "signal": -70},
        "172.17.0.3#hostapd.phy2-ap0": {"connected": false, "signal": -60},
        "172.17.0.9#hostapd.phy2-ap0": {"connected": false, "signal": -40},
    }});
    let setup = Setup {
        signal: -80,
        usteer,
        ..Setup::default()
    };
    let mut h = Harness::start(setup).await;
    let ask = h.call(0, "ubus", Duration::from_secs(5)).await.unwrap();
    assert_eq!(
        ask.args,
        json!({"path": "usteer", "method": "get_client_info", "data": {"address": CLIENT}})
    );
    let call = h.call(0, "btm", Duration::from_secs(5)).await.unwrap();
    let neighbors = [candidate(NR, 255).unwrap(), candidate(NR, 254).unwrap()];
    assert_eq!(
        call.args,
        json!({"addr": CLIENT, "neighbors": neighbors, "reason": 5,
            "imminent": true, "kick_ms": 10000, "reassoc_s": 0})
    );
    h.stop().await;
}

#[tokio::test]
async fn off_conservative_roams_and_clients_without_802_11v_ask_nothing() {
    for setup in [
        Setup {
            profile: SteeringProfile::Off,
            ..Setup::default()
        },
        // Conservative has no roam triggers.
        Setup {
            profile: SteeringProfile::Conservative,
            signal: -85,
            ..Setup::default()
        },
        Setup {
            btm: false,
            ..Setup::default()
        },
    ] {
        let mut h = Harness::start(setup).await;
        tokio::time::sleep(QUIET).await;
        h.clock.advance(Duration::from_mins(3));
        assert!(h.call(0, "btm", QUIET).await.is_none());
        h.scripts[0].drain();
        assert!(h.call(0, "ubus", QUIET).await.is_none());
        h.stop().await;
    }
}
