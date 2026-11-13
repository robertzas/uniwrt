//! Gathering over the small site of `build::tests`: nodes 1 and 2 with a 5 GHz radio on 36 at
//! 80 MHz carrying an AP and the mesh, node 2 peered with node 1, node 1's scan.

use uniwrt_core::NodeId;
use uniwrt_model::{
    facts::{NodeFacts, StationFacts, WifiIfaceMode},
    node::NodeMode,
    rf::DfsPolicy,
    site::{
        SiteConfig,
        node::{ManagedPackage, NodeConfig, RadioOverride},
        wireless::{Channel, TxPower},
    },
    wifi::{Band, ChannelWidth},
};

use super::{Gathered, Sources, gather};
use crate::{
    build::tests::{built, facts, iface, radio, scan},
    plan::{Reason, plan},
};

const ONE: NodeId = NodeId::new(1);
const TWO: NodeId = NodeId::new(2);

fn site(managed: &[NodeId]) -> SiteConfig {
    let mut site = SiteConfig::defaults(None);
    site.nodes = managed
        .iter()
        .map(|node| {
            let mut config = NodeConfig::new(*node);
            config.managed.insert(ManagedPackage::Wireless);
            config
        })
        .collect();
    site
}

fn band_channel(site: &mut SiteConfig, band: Band, channel: Channel) {
    if let Some(policy) = site.radio.bands.iter_mut().find(|p| p.band == band) {
        policy.channel = channel;
    }
}

fn run(site: &SiteConfig, nodes: &[(NodeFacts, NodeMode)]) -> Gathered {
    let nodes: Vec<(NodeId, &NodeFacts, NodeMode)> = nodes
        .iter()
        .zip(1..)
        .map(|((facts, mode), n)| (NodeId::new(n), facts, *mode))
        .collect();
    let report = built();
    let scans = [scan()];
    gather(&Sources {
        nodes: &nodes,
        report: &report,
        scans: &scans,
        site,
        dfs: DfsPolicy::NonWeather,
    })
}

fn managed() -> Vec<(NodeFacts, NodeMode)> {
    vec![(facts(1), NodeMode::Managed), (facts(2), NodeMode::Managed)]
}

#[test]
fn a_managed_radio_is_pinned_by_its_override_then_its_band() {
    let mut site = site(&[ONE, TWO]);
    band_channel(&mut site, Band::G5, Channel::Fixed(44));
    site.nodes[0].radios.push(RadioOverride {
        radio: "radio1".to_owned(),
        disabled: false,
        channel: None,
        width: Some(ChannelWidth::Mhz40),
        tx_power: Some(TxPower::Dbm(17)),
    });
    let gathered = run(&site, &managed());
    assert_eq!(gathered.dfs, DfsPolicy::NonWeather);
    // The 2.4 GHz radios are disabled.
    assert_eq!(gathered.radios.len(), 2);
    let (one, two) = (&gathered.radios[0], &gathered.radios[1]);
    assert_eq!((one.node, one.band, one.mesh), (ONE, Band::G5, true));
    assert_eq!(one.pin, Some(44));
    assert_eq!(one.pin_width, Some(ChannelWidth::Mhz40));
    assert_eq!(one.tx_power, TxPower::Dbm(17));
    assert_eq!(one.channels.len(), 6);
    assert_eq!(
        one.current.map(|c| (c.channel, c.width)),
        Some((36, ChannelWidth::Mhz80))
    );
    assert!(!one.observed);
    // The band's width doesn't pin (D-111); its power is auto.
    assert_eq!(
        (two.pin, two.pin_width, two.tx_power),
        (Some(44), None, TxPower::Auto)
    );

    // An override's auto channel frees the radio from the band's.
    site.nodes[0].radios[0].channel = Some(Channel::Auto);
    assert_eq!(run(&site, &managed()).radios[0].pin, None);
    band_channel(&mut site, Band::G5, Channel::Auto);
    assert_eq!(run(&site, &managed()).radios[1].pin, None);
}

#[test]
fn a_radio_the_controller_may_not_change_is_pinned_to_what_it_runs() {
    // Node 1 observed; node 2 managed, but the site doesn't manage its Wi-Fi.
    let site = site(&[]);
    let nodes = vec![(facts(1), NodeMode::Observe), (facts(2), NodeMode::Managed)];
    let gathered = run(&site, &nodes);
    for radio in &gathered.radios {
        assert!(radio.observed);
        assert_eq!(radio.pin, Some(36));
        assert_eq!(radio.pin_width, Some(ChannelWidth::Mhz80));
        assert_eq!(radio.tx_power, TxPower::Dbm(20));
    }
    assert_eq!(gathered.radios.len(), 2);

    // Down, it neither transmits nor can be changed: left out.
    let mut down = facts(1);
    down.radios[0].up = false;
    let gathered = run(&site, &[(down, NodeMode::Observe)]);
    assert_eq!(gathered.radios, Vec::new());
}

#[test]
fn a_disabled_override_leaves_a_managed_radio_out() {
    let mut site = site(&[ONE, TWO]);
    site.nodes[1].radios.push(RadioOverride {
        radio: "radio1".to_owned(),
        disabled: true,
        channel: None,
        width: None,
        tx_power: None,
    });
    let gathered = run(&site, &managed());
    assert_eq!(
        gathered.radios.iter().map(|r| r.node).collect::<Vec<_>>(),
        [ONE]
    );
}

#[test]
fn the_radio_beside_the_mesh_is_not_pinned_by_the_band() {
    let mut site = site(&[ONE, TWO]);
    band_channel(&mut site, Band::G5, Channel::Fixed(44));
    let mut tri = facts(1);
    let mut high = radio(1, Band::G5, None);
    high.name = "radio2".to_owned();
    high.ifaces = vec![iface(WifiIfaceMode::Ap, "phy1-ap1", "02:01:00:00:00:09")];
    tri.radios.push(high);
    let gathered = run(&site, &[(tri, NodeMode::Managed)]);
    let pins: Vec<_> = gathered
        .radios
        .iter()
        .map(|r| (r.radio.as_str(), r.mesh, r.pin))
        .collect();
    assert_eq!(pins, [("radio1", true, Some(44)), ("radio2", false, None)]);
}

#[test]
fn clients_leave_out_mesh_peers() {
    let site = site(&[ONE, TWO]);
    let mut two = facts(2);
    let mut client: StationFacts = two.stations[0].clone();
    client.mac = "0a:00:00:00:00:01".parse().unwrap();
    client.iface = "phy1-ap0".to_owned();
    client.peer_link = None;
    let gathered = run(
        &site,
        &[
            (facts(1), NodeMode::Managed),
            (two.clone(), NodeMode::Managed),
        ],
    );
    assert_eq!(gathered.radios[1].clients, 0);
    two.stations.push(client);
    let gathered = run(
        &site,
        &[(facts(1), NodeMode::Managed), (two, NodeMode::Managed)],
    );
    assert_eq!(gathered.radios[1].clients, 1);
}

#[test]
fn links_networks_and_channels_come_from_the_report_and_the_surveys() {
    let gathered = run(&site(&[ONE, TWO]), &managed());
    let link = |from, to| {
        gathered
            .links
            .iter()
            .find(|l| (l.band, l.from, l.to) == (Band::G5, from, to))
            .unwrap()
    };
    assert!(link(TWO, ONE).mesh, "node 2's live peer link to node 1");
    assert!(!link(ONE, TWO).mesh, "node 1 only scanned node 2");

    // Foreign networks and the lookalike, not node 2's own BSSs.
    let mut primaries: Vec<(NodeId, u16, i16)> = gathered
        .heard
        .iter()
        .map(|h| (h.node, h.primary, h.signal_dbm))
        .collect();
    primaries.sort_unstable();
    assert_eq!(primaries, [(ONE, 36, -80), (ONE, 44, -70), (ONE, 149, -88)]);

    let channel = |number| {
        gathered
            .channels
            .iter()
            .find(|c| c.band == Band::G5 && c.channel == number)
            .unwrap()
    };
    // The survey's 30 %, not the radio's live 12.5 %.
    assert_eq!(channel(36).busy_percent, Some(30.0));
    assert_eq!(channel(40).busy_percent, None);
    assert_eq!(
        (channel(52).busy_percent, channel(52).radar_hits),
        (Some(5.0), 1)
    );
}

#[test]
fn an_observed_radio_keeps_what_it_runs_in_the_plan() {
    let nodes = vec![(facts(1), NodeMode::Observe), (facts(2), NodeMode::Managed)];
    let gathered = run(&site(&[TWO]), &nodes);
    let planned = plan(&gathered.inputs());
    let band = &planned.bands[0];
    let one = band.radios.iter().find(|r| r.node == ONE).unwrap();
    assert!(!one.changed);
    assert_eq!(
        one.reasons,
        [Reason::Observed, Reason::MeshGroup { radios: 2 }]
    );
    // Node 2 shares the mesh, so it follows node 1's channel.
    let two = band.radios.iter().find(|r| r.node == TWO).unwrap();
    assert_eq!(
        (two.planned.channel, two.planned.width),
        (36, ChannelWidth::Mhz80)
    );
}
