//! The Wi-Fi and mesh sets on the hwsim lab (TESTING.md, Wi-Fi lab): the bundle, installed over
//! SSH, collects every set on node a (25.12, the gateway) and node b (24.10, an AP), and their
//! facts show the lab's topology. Ignored by default: `just test-wifi` starts the lab and runs
//! these with `UNIWRT_LAB_GW` and `UNIWRT_LAB_AP` (`<host:port>`) and `UNIWRT_LAB_PASSWORD`.

// Test code throughout: clippy.toml allows unwrap in tests, but not in their helpers.
#![allow(clippy::unwrap_used)]

mod openwrt;

use std::{env, time::Duration};

use openwrt::{adopt_at, link};
use uniwrt_core::MacAddr;
use uniwrt_model::facts::{DeviceKind, GatewayMode, NodeFacts, PeerLinkState, WifiIfaceMode};
use uniwrt_node::{
    AgentLaunch, Hello, NodeLink as _, facts,
    proto::ops::{Collect, Set},
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
    let mut facts = NodeFacts::default();
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
