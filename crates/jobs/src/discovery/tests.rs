//! Discovery over the canonical gateway's facts and a scripted LAN: the gateway leases to a
//! laptop, a thermostat, an unnamed device and the AP `ap-living` (which batman-adv announces
//! from originator `…:23`), and sees a mesh node `…:33` with only a Wi-Fi client behind it.

use std::{
    collections::BTreeMap,
    net::IpAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::sync::watch;
use uniwrt_core::{MacAddr, ManualClock, NodeId, Timestamp};
use uniwrt_model::{
    discovery::{DiscoveryReport, DiscoverySource, ProbeState},
    facts::NodeFacts,
    node::{Node, NodeMode, NodeRole},
};
use uniwrt_node::{ControllerKey, Credential, HostKey, Result, Target};

use super::{Discovery, FRESH};
use crate::{BoxFuture, Dial, Remote};

const GATEWAY: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../model/fixtures/facts/gateway.json"
));
const DROPBEAR: &str = "SSH-2.0-dropbear_2024.86";

/// Port 22 of each host on the LAN: a banner, nothing SSH, or refused (`Err`).
struct Lan {
    banners: BTreeMap<&'static str, std::result::Result<Option<&'static str>, ()>>,
    host_key: HostKey,
    /// Every banner and key read, by host.
    calls: Mutex<Vec<(&'static str, String)>>,
    /// Probes wait while it is closed.
    open: watch::Receiver<bool>,
}

impl Lan {
    fn new(open: watch::Receiver<bool>) -> Self {
        Self {
            banners: BTreeMap::from([
                ("10.0.0.2", Ok(Some(DROPBEAR))),
                ("10.0.0.20", Ok(Some("SSH-2.0-OpenSSH_9.9"))),
                ("10.0.0.21", Err(())),
                ("10.0.0.30", Ok(None)),
            ]),
            host_key: HostKey::parse(&ControllerKey::generate().unwrap().authorized_key().unwrap())
                .unwrap(),
            calls: Mutex::default(),
            open,
        }
    }

    fn calls(&self) -> Vec<(&'static str, String)> {
        self.calls.lock().unwrap().clone()
    }

    async fn gate(&self, call: &'static str, target: &Target) {
        self.calls.lock().unwrap().push((call, target.host.clone()));
        let _ = self.open.clone().wait_for(|open| *open).await;
    }
}

impl Dial for Lan {
    fn banner<'a>(&'a self, target: &'a Target) -> BoxFuture<'a, Result<Option<String>>> {
        Box::pin(async move {
            self.gate("banner", target).await;
            match self.banners.get(target.host.as_str()) {
                Some(Ok(banner)) => Ok(banner.map(str::to_owned)),
                _ => Err(uniwrt_node::Error::Connect {
                    target: target.to_string(),
                    source: std::io::ErrorKind::ConnectionRefused.into(),
                }),
            }
        })
    }

    fn probe<'a>(&'a self, target: &'a Target) -> BoxFuture<'a, Result<HostKey>> {
        Box::pin(async move {
            self.gate("probe", target).await;
            Ok(self.host_key.clone())
        })
    }

    fn connect<'a>(
        &'a self,
        _: &'a Target,
        _: &'a HostKey,
        _: &'a Credential,
    ) -> BoxFuture<'a, Result<Box<dyn Remote>>> {
        unreachable!("discovery never logs in")
    }
}

struct Setup {
    clock: Arc<ManualClock>,
    lan: Arc<Lan>,
    open: watch::Sender<bool>,
    discovery: Discovery,
    adopted: Vec<(Node, Option<Arc<NodeFacts>>)>,
}

fn setup(dial: bool) -> Setup {
    let clock = Arc::new(ManualClock::new(Timestamp::from_unix_secs(1_793_059_200)));
    let (open, gate) = watch::channel(true);
    let lan = Arc::new(Lan::new(gate));
    let mut facts: NodeFacts = serde_json::from_str(GATEWAY).unwrap();
    facts.leases[1].hostname = Some("OpenWrt".to_owned());
    let gateway = node(1, "gateway", "94:83:c4:00:00:10", "10.0.0.1");
    Setup {
        discovery: Discovery::new(
            dial.then(|| Arc::clone(&lan) as Arc<dyn Dial>),
            clock.clone(),
        ),
        clock,
        lan,
        open,
        adopted: vec![(gateway, Some(Arc::new(facts)))],
    }
}

fn node(id: i64, name: &str, mac: &str, host: &str) -> Node {
    Node {
        id: NodeId::new(id),
        name: name.to_owned(),
        mac: mac.parse().unwrap(),
        board_name: "test".to_owned(),
        host: host.to_owned(),
        port: 22,
        role: NodeRole::Gateway,
        mode: NodeMode::Observe,
        created_at: Timestamp::UNIX_EPOCH,
        updated_at: Timestamp::UNIX_EPOCH,
    }
}

fn mac(text: &str) -> MacAddr {
    text.parse().unwrap()
}

/// Each node as (MAC address, address, probe state).
fn summary(report: &DiscoveryReport) -> Vec<(MacAddr, Option<IpAddr>, ProbeState)> {
    report
        .nodes
        .iter()
        .map(|node| (node.mac, node.ip, node.probe))
        .collect()
}

const WAIT: Duration = Duration::from_secs(5);

#[tokio::test]
async fn reports_dropbear_servers_openwrt_names_and_mesh_nodes_known_only_by_originator() {
    let setup = setup(true);
    let report = setup.discovery.scan(&setup.adopted, WAIT).await;
    assert!(!report.scanning);
    assert_eq!(
        summary(&report),
        [
            (
                mac("96:83:c4:00:00:24"),
                Some("10.0.0.2".parse().unwrap()),
                ProbeState::Done
            ),
            (
                mac("6a:1f:2c:3d:4e:5f"),
                Some("10.0.0.21".parse().unwrap()),
                ProbeState::Unreachable
            ),
            (mac("96:83:c4:00:00:33"), None, ProbeState::Skipped),
        ]
    );
    let ap = &report.nodes[0];
    assert_eq!(ap.hostname.as_deref(), Some("ap-living"));
    assert_eq!(
        ap.sources,
        [
            DiscoverySource::Lease,
            DiscoverySource::Neighbor,
            DiscoverySource::Mesh
        ]
    );
    assert_eq!(ap.seen_by, NodeId::new(1));
    assert_eq!(ap.ssh.as_deref(), Some(DROPBEAR));
    let key = ap.host_key.as_ref().unwrap();
    assert_eq!(key.fingerprint, setup.lan.host_key.fingerprint());
    assert_eq!(key.host_key, setup.lan.host_key.to_openssh());
    assert!(ap.probed_at.is_some());
    assert_eq!(report.nodes[2].sources, [DiscoverySource::Mesh]);

    // Every leased or neighboring LAN address is probed, the WAN's never; only dropbear gives
    // its host key.
    let mut calls = setup.lan.calls();
    calls.sort();
    let calls: Vec<(&str, &str)> = calls.iter().map(|(c, h)| (*c, h.as_str())).collect();
    assert_eq!(
        calls,
        [
            ("banner", "10.0.0.2"),
            ("banner", "10.0.0.20"),
            ("banner", "10.0.0.21"),
            ("banner", "10.0.0.30"),
            ("probe", "10.0.0.2"),
        ]
    );
}

#[tokio::test]
async fn probes_hold_ten_minutes_and_stop_for_devices_gone() {
    let mut setup = setup(true);
    let first = setup.discovery.scan(&setup.adopted, WAIT).await;
    assert_eq!(setup.lan.calls().len(), 5);
    let again = setup.discovery.scan(&setup.adopted, WAIT).await;
    assert_eq!(again, first);
    assert_eq!(setup.lan.calls().len(), 5);

    setup.clock.advance(FRESH);
    // The laptop's leases ended and it left the neighbor cache.
    let laptop = mac("3c:22:fb:12:34:56");
    let facts = Arc::make_mut(setup.adopted[0].1.as_mut().unwrap());
    facts.leases.retain(|lease| lease.mac != Some(laptop));
    facts.neighbors.retain(|neighbor| neighbor.mac != laptop);
    setup.discovery.scan(&setup.adopted, WAIT).await;
    let calls = setup.lan.calls();
    assert_eq!(calls.len(), 9);
    assert!(!calls[5..].iter().any(|(_, host)| host == "10.0.0.20"));
    assert_eq!(setup.discovery.probes().len(), 3);
}

#[tokio::test(start_paused = true)]
async fn a_scan_reports_what_it_has_when_probes_are_slow() {
    let setup = setup(true);
    setup.open.send_replace(false);
    let report = setup
        .discovery
        .scan(&setup.adopted, Duration::from_secs(1))
        .await;
    assert!(report.scanning);
    // Only the OpenWrt name shows before its probe ends; the AP's originator stands for it.
    assert_eq!(
        summary(&report),
        [
            (
                mac("6a:1f:2c:3d:4e:5f"),
                Some("10.0.0.21".parse().unwrap()),
                ProbeState::Probing
            ),
            (mac("96:83:c4:00:00:23"), None, ProbeState::Probing),
            (mac("96:83:c4:00:00:33"), None, ProbeState::Skipped),
        ]
    );
    // At most 4 run at once; nothing is probed twice.
    assert_eq!(setup.lan.calls().len(), 4);
    setup.open.send_replace(true);
    let report = setup.discovery.scan(&setup.adopted, WAIT).await;
    assert!(!report.scanning);
    assert_eq!(report.nodes.len(), 3);
    assert_eq!(setup.lan.calls().len(), 5);
}

#[tokio::test]
async fn adopted_nodes_and_their_originators_are_not_discovered() {
    let mut setup = setup(true);
    // ap-living, adopted a moment ago: no facts yet.
    setup
        .adopted
        .push((node(2, "ap-living", "96:83:c4:00:00:24", "10.0.0.2"), None));
    let report = setup.discovery.scan(&setup.adopted, WAIT).await;
    assert_eq!(
        summary(&report),
        [
            (
                mac("6a:1f:2c:3d:4e:5f"),
                Some("10.0.0.21".parse().unwrap()),
                ProbeState::Unreachable
            ),
            (mac("96:83:c4:00:00:33"), None, ProbeState::Skipped),
        ]
    );
    assert!(!setup.lan.calls().iter().any(|(_, host)| host == "10.0.0.2"));
}

#[tokio::test]
async fn demo_mode_probes_nothing() {
    let setup = setup(false);
    let report = setup.discovery.scan(&setup.adopted, WAIT).await;
    assert!(!report.scanning);
    assert_eq!(
        summary(&report),
        [
            (
                mac("6a:1f:2c:3d:4e:5f"),
                Some("10.0.0.21".parse().unwrap()),
                ProbeState::Skipped
            ),
            (mac("96:83:c4:00:00:23"), None, ProbeState::Skipped),
            (mac("96:83:c4:00:00:33"), None, ProbeState::Skipped),
        ]
    );
    assert_eq!(setup.lan.calls(), []);
}
