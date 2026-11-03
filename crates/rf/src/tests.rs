//! A small site made in code: node 1 and node 2, each with a 5 GHz radio on channel 36 at 80 MHz
//! carrying an AP and a mesh point, and node 1's latest scan.

use uniwrt_core::{MacAddr, NodeId, ScanId, Timestamp};
use uniwrt_model::{
    facts::{
        Channel, DfsState, NodeFacts, PhyBand, PhyFacts, RadioFacts, StationFacts, WifiIfaceFacts,
        WifiIfaceMode,
    },
    rf::{Bss, ChannelSurvey, Scan, ScanTrigger, Security},
    wifi::{Band, ChannelWidth, WifiStandard},
};

use super::{Inputs, report};
use crate::{MatrixSource, NeighborKind, RadarHit, Span};

const AT: Timestamp = Timestamp::from_unix_ms(1_760_000_000_000);

fn mac(text: &str) -> MacAddr {
    text.parse().unwrap()
}

fn iface(mode: WifiIfaceMode, name: &str, bssid: &str) -> WifiIfaceFacts {
    WifiIfaceFacts {
        name: Some(name.to_owned()),
        section: None,
        mode,
        ssid: (mode == WifiIfaceMode::Ap).then(|| "home".to_owned()),
        mesh_id: (mode == WifiIfaceMode::Mesh).then(|| "home-mesh".to_owned()),
        bssid: Some(mac(bssid)),
        encryption: None,
        networks: Vec::new(),
        up: true,
    }
}

fn radio(n: u8, band: Band, busy: Option<f32>) -> RadioFacts {
    RadioFacts {
        name: "radio1".to_owned(),
        phy: Some("phy1".to_owned()),
        band,
        up: true,
        disabled: band == Band::G2,
        channel: Some(36),
        frequency_mhz: Some(5180),
        center_frequency_mhz: Some(5210),
        width: Some(ChannelWidth::Mhz80),
        standard: Some(WifiStandard::He),
        tx_power_dbm: Some(20),
        noise_dbm: Some(-92),
        channel_busy_percent: busy,
        country: Some("US".to_owned()),
        ifaces: vec![
            iface(
                WifiIfaceMode::Ap,
                "phy1-ap0",
                &format!("02:0{n}:00:00:00:01"),
            ),
            iface(
                WifiIfaceMode::Mesh,
                "phy1-mesh0",
                &format!("02:0{n}:00:00:00:02"),
            ),
        ],
    }
}

fn channel(number: u16, dfs_state: Option<DfsState>) -> Channel {
    Channel {
        number,
        frequency_mhz: Band::G5.frequency(number),
        dfs: dfs_state.is_some(),
        dfs_state,
        no_ir: false,
        max_tx_power_dbm: Some(23),
    }
}

fn facts(n: u8) -> NodeFacts {
    let mut facts = NodeFacts {
        collected_at: AT,
        ..NodeFacts::default()
    };
    facts.radios = vec![radio(n, Band::G5, Some(12.5)), radio(n, Band::G2, None)];
    let state = if n == 1 {
        DfsState::Usable
    } else {
        DfsState::Available
    };
    facts.phys = vec![PhyFacts {
        name: "phy1".to_owned(),
        bands: vec![PhyBand {
            band: Band::G5,
            standards: [WifiStandard::He].into(),
            widths: [ChannelWidth::Mhz80].into(),
            spatial_streams: Some(2),
            channels: vec![
                channel(36, None),
                channel(40, None),
                channel(44, None),
                channel(48, None),
                channel(52, Some(state)),
                channel(56, Some(if n == 2 { DfsState::Unavailable } else { state })),
            ],
        }],
    }];
    if n == 2 {
        let peer: StationFacts = serde_json::from_value(serde_json::json!({
            "mac": "02:01:00:00:00:02", "iface": "phy1-mesh0", "signal_dbm": -55,
            "connected_s": 600, "inactive_ms": 10, "rx_rate": null, "tx_rate": null,
            "rx_bytes": 0, "tx_bytes": 0, "rx_packets": 0, "tx_packets": 0, "tx_retries": null,
            "tx_failed": null, "expected_throughput_kbps": null, "rx_airtime_us": null,
            "tx_airtime_us": null, "peer_link": "established", "authorized": true,
            "standard": null, "capabilities": []
        }))
        .unwrap();
        facts.stations = vec![peer];
    }
    facts
}

fn bss(
    bssid: &str,
    ssid: &str,
    channel: u16,
    width: ChannelWidth,
    center: u16,
    signal: i16,
) -> Bss {
    Bss {
        bssid: mac(bssid),
        ssid: Some(ssid.to_owned()),
        mesh_id: None,
        channel,
        frequency_mhz: Band::G5.frequency(channel),
        center_frequency_mhz: Band::G5.frequency(center),
        width,
        signal_dbm: signal,
        standard: WifiStandard::Vht,
        security: Security::Wpa2,
        stations: None,
        utilization_percent: None,
    }
}

fn scan() -> Scan {
    let survey = |number, busy| ChannelSurvey {
        channel: number,
        frequency_mhz: Band::G5.frequency(number),
        noise_dbm: Some(-95),
        busy_percent: Some(busy),
        dwell_ms: Some(110),
    };
    let mut mesh = bss("02:02:00:00:00:02", "", 36, ChannelWidth::Mhz80, 42, -57);
    mesh.ssid = None;
    mesh.mesh_id = Some("home-mesh".to_owned());
    Scan {
        id: ScanId::new(1),
        node: NodeId::new(1),
        radio: "radio1".to_owned(),
        band: Band::G5,
        at: AT,
        trigger: ScanTrigger::Scheduled,
        duration_ms: 3_000,
        bsss: vec![
            bss(
                "02:02:00:00:00:01",
                "home",
                36,
                ChannelWidth::Mhz80,
                42,
                -58,
            ),
            mesh,
            bss(
                "0c:00:00:00:00:01",
                "cafe",
                44,
                ChannelWidth::Mhz40,
                46,
                -70,
            ),
            bss(
                "0c:00:00:00:00:02",
                "far",
                149,
                ChannelWidth::Mhz20,
                149,
                -88,
            ),
            bss(
                "02:09:00:00:00:01",
                "home",
                36,
                ChannelWidth::Mhz20,
                36,
                -80,
            ),
        ],
        channels: vec![survey(36, 30.0), survey(52, 5.0)],
    }
}

fn built() -> crate::RfReport {
    let (one, two) = (facts(1), facts(2));
    let nodes = [(NodeId::new(1), &one), (NodeId::new(2), &two)];
    let scans = [scan()];
    let radar = [RadarHit {
        node: NodeId::new(1),
        at: AT,
        span: Span::new(5290, ChannelWidth::Mhz80),
    }];
    let inputs = Inputs {
        nodes: &nodes,
        scans: &scans,
        radar: &radar,
    };
    report(&inputs, |bssid| {
        (bssid == mac("0c:00:00:00:00:01")).then(|| "Acme".to_owned())
    })
}

#[test]
fn bands_are_those_of_enabled_radios_and_scans() {
    let bands: Vec<_> = built().bands.iter().map(|b| b.band).collect();
    assert_eq!(bands, [Band::G5]);
}

#[test]
fn a_radio_counts_the_foreign_networks_its_node_hears_on_its_channel() {
    let band = &built().bands[0];
    let scanned = &band.radios[0];
    let found: Vec<_> = scanned
        .on_channel
        .iter()
        .map(|o| (o.bssid.to_string(), o.co_channel, o.overlap))
        .collect();
    // cafe covers half of 36–48, the lookalike a quarter; far is elsewhere, and own BSSs never
    // count.
    assert_eq!(
        found,
        [
            ("0c:00:00:00:00:01".to_owned(), false, 0.5),
            ("02:09:00:00:00:01".to_owned(), true, 0.25),
        ]
    );
    assert!(scanned.interference > 0.0 && scanned.interference < 1.0);
    // Node 2 has not scanned: nothing counts against it.
    assert_eq!(band.radios[1].on_channel.len(), 0);
    assert_eq!(band.radios[1].interference, 0.0);
}

#[test]
fn networks_are_own_lookalike_or_foreign_most_disruptive_first() {
    let band = &built().bands[0];
    let found: Vec<_> = band
        .networks
        .iter()
        .map(|n| (n.bss.bssid.to_string(), n.kind, n.node))
        .collect();
    assert_eq!(
        found[0],
        ("0c:00:00:00:00:01".to_owned(), NeighborKind::Foreign, None)
    );
    assert_eq!(found[1].1, NeighborKind::Lookalike);
    assert!(found.contains(&(
        "02:02:00:00:00:01".to_owned(),
        NeighborKind::Own,
        Some(NodeId::new(2))
    )));
    assert_eq!(band.networks[0].vendor.as_deref(), Some("Acme"));
    assert_eq!(band.networks[0].heard_by[0].node, NodeId::new(1));
}

#[test]
fn the_matrix_prefers_live_mesh_links_and_else_the_strongest_scanned_bss() {
    let band = &built().bands[0];
    let found: Vec<_> = band
        .matrix
        .iter()
        .map(|e| (e.from.get(), e.to.get(), e.signal_dbm, e.source))
        .collect();
    assert_eq!(
        found,
        [
            (1, 2, -57, MatrixSource::Scan),
            (2, 1, -55, MatrixSource::Mesh)
        ]
    );
}

#[test]
fn channels_tell_their_state_occupancy_and_users() {
    let band = &built().bands[0];
    let by = |number: u16| band.channels.iter().find(|c| c.channel == number).unwrap();
    let ch36 = by(36);
    // Live from the radios, not the scan's 30 %.
    assert_eq!((ch36.busy_percent, ch36.noise_dbm), (Some(12.5), Some(-92)));
    assert_eq!(ch36.ours.len(), 2);
    assert!(ch36.ours.iter().all(|u| u.primary));
    assert_eq!(ch36.networks, 1, "the lookalike");
    let ch44 = by(44);
    assert_eq!((ch44.networks, ch44.strongest_dbm), (1, Some(-70)));
    assert!(ch44.ours.iter().all(|u| !u.primary));
    let ch52 = by(52);
    assert!(ch52.dfs && ch52.allowed);
    assert_eq!(ch52.dfs_state, Some(DfsState::Usable));
    assert_eq!((ch52.radar_hits, ch52.last_radar), (1, Some(AT)));
    assert_eq!(ch52.busy_percent, Some(5.0));
    assert_eq!(ch52.ours.len(), 0);
    assert_eq!(by(56).dfs_state, Some(DfsState::Unavailable));
    assert_eq!(by(48).radar_hits, 0);
    let ch149 = by(149);
    assert!(!ch149.allowed && ch149.networks == 1);
    let numbers: Vec<_> = band.channels.iter().map(|c| c.channel).collect();
    assert_eq!(numbers, [36, 40, 44, 48, 52, 56, 149]);
}
