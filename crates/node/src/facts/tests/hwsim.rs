//! The parsers against captures from the hwsim lab (`scripts/wifi-lab.sh`, TESTING.md): real
//! hostapd, `wpa_supplicant`, nl80211, netifd, dnsmasq, odhcpd and batman-adv output. Node a
//! (25.12) has a 2g AP on phy1 and a 5g AP and mesh point on phy2; node b (24.10) meshes with it
//! over the air and a wired backhaul (`bh0`); client c (02:00:00:00:04:00 on a's 2g AP,
//! 02:00:00:00:05:00 on b's 5g AP) leases its addresses from a.

use std::collections::BTreeSet;

use uniwrt_core::MacAddr;
use uniwrt_model::{
    facts::{
        BatmanAlgorithm, DeviceKind, DfsState, GatewayMode, NodeFacts, PeerLinkState,
        StationCapability, WifiIfaceMode,
    },
    wifi::{Band, ChannelWidth, WifiStandard},
};

use super::facts_of;
use crate::{facts::SetFacts, proto::ops::Collected};

const GATEWAY: &str = include_str!("../../../fixtures/collect/hwsim-a-25.12.6.json");
const AP: &str = include_str!("../../../fixtures/collect/hwsim-b-24.10.8.json");
const NEIGHBORS: &str = include_str!("../../../fixtures/collect/hwsim-a-25.12.6-neighbors.json");

fn mac(text: &str) -> MacAddr {
    text.parse().unwrap()
}

fn radio<'a>(facts: &'a NodeFacts, name: &str) -> &'a uniwrt_model::facts::RadioFacts {
    facts
        .radios
        .iter()
        .find(|radio| radio.name == name)
        .unwrap()
}

#[test]
fn phys_report_their_bands_standards_and_regulatory_channels() {
    let facts = facts_of(GATEWAY);
    let names: Vec<_> = facts.phys.iter().map(|phy| phy.name.as_str()).collect();
    assert_eq!(names, ["phy1", "phy2"]);
    let bands: Vec<_> = facts.phys[0].bands.iter().map(|band| band.band).collect();
    assert_eq!(bands, [Band::G2, Band::G5, Band::G6]);
    let five = &facts.phys[0].bands[1];
    let standards = [
        WifiStandard::Legacy,
        WifiStandard::Ht,
        WifiStandard::Vht,
        WifiStandard::He,
    ];
    assert!(
        five.standards.is_superset(&standards.into()),
        "{:?}",
        five.standards
    );
    let widths = [
        ChannelWidth::Mhz20,
        ChannelWidth::Mhz40,
        ChannelWidth::Mhz80,
        ChannelWidth::Mhz160,
    ];
    assert!(five.widths.is_superset(&widths.into()), "{:?}", five.widths);
    assert_eq!(five.spatial_streams, Some(2));
    // US: 36 allows 23 dBm; 52 needs radar detection and has not had its CAC.
    let channel = |number| five.channels.iter().find(|c| c.number == number).unwrap();
    assert_eq!(
        (channel(36).max_tx_power_dbm, channel(36).dfs),
        (Some(23), false)
    );
    assert_eq!(
        (channel(52).dfs, channel(52).dfs_state),
        (true, Some(DfsState::Usable))
    );
    assert!(facts.phys[0].bands[0].widths.contains(&ChannelWidth::Mhz40));
    assert!(
        !facts.phys[0].bands[0]
            .standards
            .contains(&WifiStandard::Vht)
    );
}

#[test]
fn radios_join_netifd_and_nl80211() {
    let facts = facts_of(GATEWAY);
    let two = radio(&facts, "radio0");
    assert_eq!(two.phy.as_deref(), Some("phy1"));
    assert_eq!(
        (two.band, two.channel, two.frequency_mhz),
        (Band::G2, Some(1), Some(2412))
    );
    assert_eq!(
        (two.width, two.standard),
        (Some(ChannelWidth::Mhz20), Some(WifiStandard::He))
    );
    assert_eq!(
        (two.country.as_deref(), two.up, two.noise_dbm),
        (Some("US"), true, Some(-92))
    );
    let five = radio(&facts, "radio1");
    assert_eq!(
        (five.channel, five.center_frequency_mhz),
        (Some(36), Some(5210))
    );
    assert_eq!(
        (five.width, five.tx_power_dbm),
        (Some(ChannelWidth::Mhz80), Some(23))
    );
    let modes: Vec<_> = five.ifaces.iter().map(|iface| iface.mode).collect();
    assert_eq!(modes, [WifiIfaceMode::Ap, WifiIfaceMode::Mesh]);
    let ap = &five.ifaces[0];
    assert_eq!(ap.name.as_deref(), Some("phy2-ap0"));
    assert_eq!(
        (ap.ssid.as_deref(), ap.bssid),
        (Some("uniwrt-lab"), Some(mac("02:00:00:00:01:00")))
    );
    assert_eq!(
        (ap.encryption.as_deref(), &ap.networks[..]),
        (Some("sae-mixed"), &["mesh".to_owned()][..])
    );
    let mesh = &five.ifaces[1];
    assert_eq!(mesh.mesh_id.as_deref(), Some("uniwrt-lab-mesh"));
    assert!(mesh.up && mesh.ssid.is_none());
    // The radio without a 2g client on b still has its phy, from its interface.
    assert_eq!(radio(&facts_of(AP), "radio1").phy.as_deref(), Some("phy4"));
}

#[test]
fn stations_carry_capabilities_and_mesh_peer_links() {
    for (json, client_iface, client, peer_iface) in [
        (GATEWAY, "phy1-ap0", "02:00:00:00:04:00", "phy2-mesh0"),
        (AP, "phy3-ap0", "02:00:00:00:05:00", "phy3-mesh0"),
    ] {
        let facts = facts_of(json);
        let station = |iface: &str| facts.stations.iter().find(|s| s.iface == iface).unwrap();
        let client_facts = station(client_iface);
        assert_eq!(client_facts.mac, mac(client));
        assert!(client_facts.authorized && client_facts.peer_link.is_none());
        assert_eq!(client_facts.standard, Some(WifiStandard::He));
        let expected = BTreeSet::from([
            StationCapability::RadioMeasurement,
            StationCapability::BssTransition,
            StationCapability::ProtectedManagementFrames,
            StationCapability::Wmm,
        ]);
        assert_eq!(client_facts.capabilities, expected);
        assert!(client_facts.signal_dbm.is_some() && client_facts.tx_rate.is_some());
        let peer = station(peer_iface);
        assert_eq!(peer.peer_link, Some(PeerLinkState::Established));
        assert!(peer.expected_throughput_kbps.is_some() && peer.rx_airtime_us.is_some());
    }
}

#[test]
fn leases_come_from_dnsmasq_and_odhcpd() {
    let facts = facts_of(GATEWAY);
    let v4: BTreeSet<_> = facts
        .leases
        .iter()
        .filter(|lease| lease.ip.is_ipv4())
        .map(|lease| lease.mac.unwrap())
        .collect();
    assert_eq!(
        v4,
        [mac("02:00:00:00:04:00"), mac("02:00:00:00:05:00")].into()
    );
    assert!(
        facts
            .leases
            .iter()
            .all(|lease| lease.expires_in_s.is_some())
    );
    let v6 = facts
        .leases
        .iter()
        .find(|lease| lease.ip.is_ipv6())
        .unwrap();
    // OpenWrt's DHCPv6 client sends a DUID-UUID, which has no MAC address.
    assert_eq!((v6.device.as_deref(), v6.mac), (Some("br-mesh"), None));
    assert_eq!(v6.hostname.as_deref(), Some("lab-client"));
    assert_eq!(facts_of(AP).leases, []);
}

#[test]
fn neighbors_and_the_fdb_place_the_clients() {
    let facts = facts_of(GATEWAY);
    let neighbor = facts
        .neighbors
        .iter()
        .find(|n| n.mac == mac("02:00:00:00:04:00"))
        .unwrap();
    assert_eq!(neighbor.device, "br-mesh");
    let port = |client| {
        let entry = facts.fdb.iter().find(|e| e.mac == mac(client)).unwrap();
        (entry.bridge.as_str(), entry.port.as_str())
    };
    assert_eq!(port("02:00:00:00:04:00"), ("br-mesh", "phy1-ap0"));
    assert_eq!(port("02:00:00:00:05:00"), ("br-mesh", "bat0"));
}

#[test]
fn batman_shows_the_mesh_from_both_ends() {
    let gateway = facts_of(GATEWAY);
    let mesh = &gateway.batman[0];
    assert_eq!(
        (mesh.mesh_if.as_str(), mesh.algorithm),
        ("bat0", Some(BatmanAlgorithm::BatmanIv))
    );
    assert_eq!(mesh.gw_mode, Some(GatewayMode::Server));
    let hard_ifs: BTreeSet<_> = mesh
        .hard_ifs
        .iter()
        .map(|h| (h.name.as_str(), h.active))
        .collect();
    assert_eq!(hard_ifs, [("bh0", true), ("phy2-mesh0", true)].into());
    let neighbor_ifs: BTreeSet<_> = mesh.neighbors.iter().map(|n| n.hard_if.as_str()).collect();
    assert_eq!(neighbor_ifs, ["bh0", "phy2-mesh0"].into());
    assert!(mesh.originators.iter().any(|o| o.best && o.tq.is_some()));
    // c's 5g uplink sits behind b: it reached a's DHCP server through the mesh.
    assert!(
        mesh.remote_clients
            .iter()
            .any(|c| c.mac == mac("02:00:00:00:05:00") && c.wifi)
    );
    let bat0 = gateway.devices.iter().find(|d| d.name == "bat0").unwrap();
    assert_eq!(bat0.kind, DeviceKind::Batman);

    let ap = &facts_of(AP).batman[0];
    assert_eq!(ap.gw_mode, Some(GatewayMode::Off));
    assert!(
        ap.local_clients
            .iter()
            .any(|c| c.mac == mac("02:00:00:00:05:00") && c.wifi)
    );
    let gateway = &ap.gateways[0];
    assert_eq!(
        (gateway.down_kbps, gateway.up_kbps),
        (Some(10_000), Some(2_000))
    );
}

/// Without rtnetlink, /proc/net/arp gives the same IPv4 neighbors, and `brforward` the same
/// forwarding database (without VLANs), less the entries that have expired but are not yet
/// collected: `brforward` skips those, and the rtnetlink dump keeps them.
#[test]
fn without_rtnetlink_the_caches_read_the_same() {
    let both: serde_json::Map<String, serde_json::Value> = serde_json::from_str(NEIGHBORS).unwrap();
    let parse = |key: &str| {
        let collected: Collected = serde_json::from_value(both[key].clone()).unwrap();
        let raw = &collected.sets[&crate::proto::ops::Set::Neighbors];
        let Ok(SetFacts::Neighbors { neighbors, fdb }) =
            SetFacts::parse(crate::proto::ops::Set::Neighbors, raw)
        else {
            panic!("{key}");
        };
        let neighbors: BTreeSet<_> = neighbors
            .into_iter()
            .filter(|n| n.ip.is_ipv4())
            .map(|n| (n.ip, n.mac, n.device))
            .collect();
        let fdb: BTreeSet<_> = fdb
            .into_iter()
            .map(|e| (e.mac.octets(), e.bridge, e.port))
            .collect();
        (neighbors, fdb)
    };
    let (rtnl, proc) = (parse("rtnl"), parse("proc"));
    assert!(!rtnl.0.is_empty());
    assert_eq!(proc.0, rtnl.0);
    assert!(proc.1.is_subset(&rtnl.1));
    for (client, port) in [
        ("02:00:00:00:04:00", "phy1-ap0"),
        ("02:00:00:00:05:00", "bat0"),
    ] {
        let entry = (mac(client).octets(), "br-mesh".to_owned(), port.to_owned());
        assert!(proc.1.contains(&entry), "{client}");
    }
}
