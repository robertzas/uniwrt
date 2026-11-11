//! The Wi-Fi and mesh sets on the hwsim lab (TESTING.md, Wi-Fi lab): the bundle, installed over
//! SSH, collects every set on node a (25.12, the gateway) and node b (24.10, an AP), and their
//! facts show the lab's topology; the gateway pushes its client's comings and goings; and each
//! node's scan hears the other's access points and mesh point; the `bss` op turns an access point
//! off and on again on both releases. The
//! tests run one at a time, since one of them disconnects the client. Ignored by default: `just test-wifi` starts the lab and runs
//! these with `UNIWRT_LAB_GW` and `UNIWRT_LAB_AP` (`<host:port>`) and `UNIWRT_LAB_PASSWORD`.

// Test code throughout: clippy.toml allows unwrap in tests, but not in their helpers.
#![allow(clippy::unwrap_used)]

mod openwrt;

use std::{env, time::Duration};

use openwrt::{adopt_at, link, next_push};
use serde_json::json;
use tokio::sync::broadcast;
use uniwrt_core::MacAddr;
use uniwrt_model::{
    event::{LeaseChange, NodeEvent, StationChange, StationEvent},
    facts::{DeviceKind, GatewayMode, NodeFacts, PeerLinkState, WifiIfaceMode},
    rf::Security,
    wifi::{Band, ChannelWidth, WifiStandard},
};
use uniwrt_node::{
    AgentEvent, AgentLaunch, Hello, NodeLink as _,
    events::Push,
    facts,
    proto::ops::{Bss, Collect, Exec, Scan, Set, Subscribe, Topic, Ubus},
};

const LONG: Duration = Duration::from_secs(30);
/// c's uplinks: to a's 2g AP, and to b's 5g AP.
const CLIENT_2G: &str = "02:00:00:00:04:00";
const CLIENT_5G: &str = "02:00:00:00:05:00";

fn mac(text: &str) -> MacAddr {
    text.parse().unwrap()
}

/// Adopts the lab node in `$var` and collects every set twice: the second round has the CPU and
/// channel-busy shares, which the first only takes the baseline for.
async fn collect(var: &str) -> (Hello, NodeFacts) {
    let address = env::var(var).unwrap_or_else(|_| panic!("{var}=<host:port>"));
    let password = env::var("UNIWRT_LAB_PASSWORD").expect("UNIWRT_LAB_PASSWORD");
    let (_, node) = adopt_at(&address, &password).await;
    let (link, hello) = link(&node, AgentLaunch::Bundle).await;
    // A bridge forgets a MAC address after 5 idle minutes: reaching every client first lets the
    // forwarding databases know them, however long the lab has been up.
    let mut facts = NodeFacts::default();
    let leases = link
        .request(
            &Collect {
                sets: vec![Set::Leases],
            },
            LONG,
        )
        .await
        .unwrap();
    assert_eq!(facts::apply(&leases, &mut facts), []);
    for lease in facts.leases.iter().filter(|lease| lease.ip.is_ipv4()) {
        let ping = Exec::new(["ping", "-c", "1", "-W", "2", &lease.ip.to_string()]);
        link.request(&ping, LONG).await.unwrap();
    }
    for _ in 0..2 {
        let collected = link.request(&Collect::all(), LONG).await.unwrap();
        assert_eq!(facts::apply(&collected, &mut facts), []);
        tokio::time::sleep(Duration::from_millis(1200)).await;
    }
    link.shutdown().await;
    (hello, facts)
}

#[tokio::test]
#[ignore = "needs the hwsim lab: just test-wifi"]
async fn the_gateway_sees_its_radios_clients_leases_and_mesh() {
    let (hello, facts) = collect("UNIWRT_LAB_GW").await;
    assert!(hello.caps.events, "Wi-Fi images have ucode-mod-uloop");
    for set in Set::ALL {
        assert!(hello.caps.has_set(set), "{set}");
    }
    assert_eq!(facts.phys.len(), 2);
    let channels: Vec<_> = facts.radios.iter().map(|radio| radio.channel).collect();
    assert_eq!(channels, [Some(1), Some(36)]);
    assert!(
        facts
            .radios
            .iter()
            .all(|radio| radio.up && radio.phy.is_some())
    );
    let mesh_iface = facts.radios[1]
        .ifaces
        .iter()
        .find(|iface| iface.mode == WifiIfaceMode::Mesh)
        .unwrap();
    let client = facts
        .stations
        .iter()
        .find(|s| s.mac == mac(CLIENT_2G))
        .unwrap();
    assert!(
        client.authorized && client.iface.ends_with("-ap0"),
        "{client:?}"
    );
    let peer = facts
        .stations
        .iter()
        .find(|s| Some(&s.iface) == mesh_iface.name.as_ref())
        .unwrap();
    assert_eq!(peer.peer_link, Some(PeerLinkState::Established));
    let leased: Vec<_> = facts.leases.iter().filter_map(|lease| lease.mac).collect();
    assert!(
        leased.contains(&mac(CLIENT_2G)) && leased.contains(&mac(CLIENT_5G)),
        "{leased:?}"
    );
    assert!(
        facts
            .fdb
            .iter()
            .any(|e| e.mac == mac(CLIENT_5G) && e.port == "bat0")
    );
    let mesh = &facts.batman[0];
    assert_eq!(mesh.gw_mode, Some(GatewayMode::Server));
    // b is a neighbor over the wired backhaul and over the air.
    assert_eq!(mesh.neighbors.len(), 2, "{:?}", mesh.neighbors);
    assert!(mesh.remote_clients.iter().any(|c| c.mac == mac(CLIENT_5G)));
    let bat0 = facts.devices.iter().find(|d| d.name == "bat0").unwrap();
    assert_eq!(bat0.kind, DeviceKind::Batman);
}

#[tokio::test]
#[ignore = "needs the hwsim lab: just test-wifi"]
async fn the_ap_sees_the_gateway_through_the_mesh() {
    let (hello, facts) = collect("UNIWRT_LAB_AP").await;
    assert_eq!(hello.release.as_deref(), Some("24.10.8"));
    let client = facts
        .stations
        .iter()
        .find(|s| s.mac == mac(CLIENT_5G))
        .unwrap();
    assert!(client.authorized, "{client:?}");
    assert!(facts.leases.is_empty(), "DHCP is the gateway's");
    let mesh = &facts.batman[0];
    assert_eq!(mesh.gw_mode, Some(GatewayMode::Off));
    assert_eq!(mesh.gateways.len(), 1, "{:?}", mesh.gateways);
    assert!(
        mesh.local_clients
            .iter()
            .any(|c| c.mac == mac(CLIENT_5G) && c.wifi)
    );
}

async fn station(
    pushes: &mut broadcast::Receiver<AgentEvent>,
    change: StationChange,
) -> StationEvent {
    next_push(pushes, LONG, |push| match push {
        Push::Event(NodeEvent::Station(s)) if s.mac == mac(CLIENT_2G) && s.change == change => {
            Some(s)
        }
        _ => None,
    })
    .await
}

/// Client c kicked off the gateway's 2g AP leaves, comes back and renews its lease. When the AP's
/// radio restarts, its hostapd object goes and comes back, and the agent follows it. Waits for
/// the client to be back, so the other tests find it connected.
#[tokio::test]
#[ignore = "needs the hwsim lab: just test-wifi"]
async fn the_gateway_pushes_station_and_lease_events_across_radio_restarts() {
    let address = env::var("UNIWRT_LAB_GW").expect("UNIWRT_LAB_GW=<host:port>");
    let password = env::var("UNIWRT_LAB_PASSWORD").expect("UNIWRT_LAB_PASSWORD");
    let (_, node) = adopt_at(&address, &password).await;
    let (link, _) = link(&node, AgentLaunch::Bundle).await;
    let where_is = Collect {
        sets: vec![Set::Radios, Set::Stations],
    };
    let mut facts = NodeFacts::default();
    assert_eq!(
        facts::apply(&link.request(&where_is, LONG).await.unwrap(), &mut facts),
        []
    );
    let iface = facts
        .stations
        .iter()
        .find(|s| s.mac == mac(CLIENT_2G))
        .unwrap()
        .iface
        .clone();
    let radio = facts
        .radios
        .iter()
        .find(|r| r.ifaces.iter().any(|i| i.name.as_ref() == Some(&iface)))
        .unwrap()
        .name
        .clone();

    let mut pushes = link.events();
    let topics = Subscribe {
        topics: vec![Topic::Hostapd, Topic::Dhcp],
    };
    let subscribed = link.request(&topics, LONG).await.unwrap();
    assert_eq!(subscribed.topics, [Topic::Dhcp, Topic::Hostapd]);
    let mut kick = Ubus::new(format!("hostapd.{iface}"), "del_client");
    let args = json!({"addr": CLIENT_2G, "reason": 5, "deauth": true, "ban_time": 0});
    kick.data = args.as_object().unwrap().clone();
    link.request(&kick, LONG).await.unwrap();
    let left = station(&mut pushes, StationChange::Disassociated).await;
    assert_eq!(left.iface, iface);
    let asked = station(&mut pushes, StationChange::AssocRequest).await;
    assert!(
        asked.signal_dbm.is_some() && asked.freq_mhz.is_some(),
        "{asked:?}"
    );
    let back = station(&mut pushes, StationChange::Authorized).await;
    assert!(back.auth_alg.is_some(), "{back:?}");
    next_push(&mut pushes, LONG, |push| match push {
        Push::Event(NodeEvent::Lease(lease))
            if lease.mac == Some(mac(CLIENT_2G)) && lease.change == LeaseChange::Acked =>
        {
            Some(())
        }
        _ => None,
    })
    .await;

    let wifi = |action: &str| Exec::new(["wifi", action, radio.as_str()]);
    assert!(link.request(&wifi("down"), LONG).await.unwrap().success());
    station(&mut pushes, StationChange::Disassociated).await;
    assert!(link.request(&wifi("up"), LONG).await.unwrap().success());
    let back = station(&mut pushes, StationChange::Authorized).await;
    assert_eq!(back.iface, iface);
    link.shutdown().await;
}

/// Turns the lab node in `$var`'s access point that `client` uses off with the `bss` op, then on
/// again (D-083): off, its section stays in netifd's status with the interface down; on, the
/// client comes back to it.
async fn bss_off_and_on(var: &str, client: &str) {
    let address = env::var(var).unwrap_or_else(|_| panic!("{var}=<host:port>"));
    let password = env::var("UNIWRT_LAB_PASSWORD").expect("UNIWRT_LAB_PASSWORD");
    let (_, node) = adopt_at(&address, &password).await;
    let (link, _) = link(&node, AgentLaunch::Bundle).await;
    let where_is = Collect {
        sets: vec![Set::Radios, Set::Stations],
    };
    let look = async || {
        let mut facts = NodeFacts::default();
        let collected = link.request(&where_is, LONG).await.unwrap();
        assert_eq!(facts::apply(&collected, &mut facts), []);
        facts
    };
    let facts = look().await;
    let iface = facts
        .stations
        .iter()
        .find(|s| s.mac == mac(client))
        .unwrap_or_else(|| panic!("{client} on {var}: {:?}", facts.stations))
        .iface
        .clone();
    let (radio, section) = facts
        .radios
        .iter()
        .find_map(|r| {
            let i = r.ifaces.iter().find(|i| i.name.as_ref() == Some(&iface))?;
            Some((r.clone(), i.section.clone().unwrap()))
        })
        .unwrap();
    let phy = radio.phy.clone().unwrap();

    let off = Bss {
        phy: phy.clone(),
        off: vec![iface.clone()],
    };
    let set = link.request(&off, LONG).await.unwrap();
    assert_eq!(set.radios.len(), 1, "{set:?}");
    assert_eq!(set.radios[0].off, std::slice::from_ref(&iface));
    assert!(!set.radios[0].on.contains(&iface));
    let facts = look().await;
    let bss = facts
        .radios
        .iter()
        .flat_map(|r| &r.ifaces)
        .find(|i| i.section.as_ref() == Some(&section))
        .unwrap_or_else(|| panic!("{section} is gone: {:?}", facts.radios));
    assert!(!bss.up, "{bss:?}");
    assert!(!facts.stations.iter().any(|s| s.mac == mac(client)));

    let on = Bss { phy, off: vec![] };
    let set = link.request(&on, LONG).await.unwrap();
    assert!(set.radios[0].on.contains(&iface) && set.radios[0].off.is_empty());
    for _ in 0..30 {
        let facts = look().await;
        let up = facts
            .radios
            .iter()
            .flat_map(|r| &r.ifaces)
            .any(|i| i.section.as_ref() == Some(&section) && i.up);
        if up && facts.stations.iter().any(|s| s.mac == mac(client)) {
            link.shutdown().await;
            return;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    panic!("{client} did not come back to {var}'s {section}");
}

#[tokio::test]
#[ignore = "needs the hwsim lab: just test-wifi"]
async fn the_bss_op_turns_one_access_point_off_and_on_again() {
    // a (25.12): the 2g AP; b (24.10): the 5g AP, c's way to the gateway through the mesh.
    bss_off_and_on("UNIWRT_LAB_GW", CLIENT_2G).await;
    bss_off_and_on("UNIWRT_LAB_AP", CLIENT_5G).await;
}

/// Scans `band` through the lab node in `$var`'s access point on it.
async fn scan(var: &str, band: Band) -> uniwrt_model::rf::Heard {
    let address = env::var(var).unwrap_or_else(|_| panic!("{var}=<host:port>"));
    let password = env::var("UNIWRT_LAB_PASSWORD").expect("UNIWRT_LAB_PASSWORD");
    let (_, node) = adopt_at(&address, &password).await;
    let (link, _) = link(&node, AgentLaunch::Bundle).await;
    let sets = Collect {
        sets: vec![Set::Phys, Set::Radios],
    };
    let collected = link.request(&sets, LONG).await.unwrap();
    let mut facts = NodeFacts::default();
    assert_eq!(facts::apply(&collected, &mut facts), []);
    let radio = facts.radios.iter().find(|r| r.band == band).unwrap();
    let ap = radio
        .ifaces
        .iter()
        .find(|i| i.mode == WifiIfaceMode::Ap)
        .and_then(|i| i.name.clone())
        .unwrap();
    let phy = facts
        .phys
        .iter()
        .find(|p| Some(&p.name) == radio.phy.as_ref())
        .unwrap();
    let freqs = phy
        .bands
        .iter()
        .filter(|b| b.band == band)
        .flat_map(|b| b.channels.iter().map(|c| c.frequency_mhz))
        .collect();
    let scanned = link.request(&Scan::new(ap, freqs), LONG).await.unwrap();
    link.shutdown().await;
    facts::scan::heard(&scanned, band)
}

#[tokio::test]
#[ignore = "needs the hwsim lab: just test-wifi"]
async fn each_node_hears_the_others_networks() {
    // a (25.12) hears b's 5 GHz AP (HE80 on 36, sae-mixed) and its mesh point.
    let heard = scan("UNIWRT_LAB_GW", Band::G5).await;
    let ap = heard
        .bsss
        .iter()
        .find(|b| b.ssid.as_deref() == Some("uniwrt-lab") && b.mesh_id.is_none())
        .unwrap_or_else(|| panic!("{heard:?}"));
    assert_eq!((ap.channel, ap.width), (36, ChannelWidth::Mhz80));
    assert_eq!(ap.center_frequency_mhz, 5210);
    assert_eq!(
        (ap.standard, ap.security),
        (WifiStandard::He, Security::Wpa2Wpa3)
    );
    assert!((-100..0).contains(&ap.signal_dbm), "{ap:?}");
    assert!(
        heard
            .bsss
            .iter()
            .any(|b| b.mesh_id.as_deref() == Some("uniwrt-lab-mesh")),
        "{heard:?}"
    );
    assert!(heard.duration_ms > 0);
    // b (24.10) hears a's 2.4 GHz AP on channel 1.
    let heard = scan("UNIWRT_LAB_AP", Band::G2).await;
    assert!(
        heard
            .bsss
            .iter()
            .any(|b| b.ssid.as_deref() == Some("uniwrt-lab") && b.channel == 1),
        "{heard:?}"
    );
}
