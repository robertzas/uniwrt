//! The peers' status from the gateway's facts: matched by key, connected within three minutes of
//! the collection, handshakes moved onto the controller's clock.

use uniwrt_core::Timestamp;
use uniwrt_model::{
    facts::{NodeFacts, WireguardFacts, WireguardPeerFacts},
    site::vpn::WireGuard,
};

use super::status;

fn server() -> WireGuard {
    serde_json::from_value(serde_json::json!({
        "enabled": true, "port": 51820, "address": "10.0.250.1/24",
        "endpoint": { "kind": "host", "host": "vpn.example.com" }, "dns": true,
        "private_key": "config/vpn/wireguard/private_key", "public_key": "c2VydmVy",
        "peers": [
            { "id": "phone", "name": "Phone", "enabled": true, "address": "10.0.250.2",
              "public_key": "cGhvbmU=", "private_key": null, "preshared_key": null,
              "role": { "kind": "client", "full_tunnel": true } },
            { "id": "laptop", "name": "Laptop", "enabled": true, "address": "10.0.250.3",
              "public_key": "bGFwdG9w", "private_key": null, "preshared_key": null,
              "role": { "kind": "client", "full_tunnel": false } },
            { "id": "tablet", "name": "Tablet", "enabled": true, "address": "10.0.250.4",
              "public_key": "dGFibGV0", "private_key": null, "preshared_key": null,
              "role": { "kind": "client", "full_tunnel": false } }
        ]
    }))
    .unwrap()
}

fn seen(key: &str, handshake: Option<i64>) -> WireguardPeerFacts {
    WireguardPeerFacts {
        public_key: key.to_owned(),
        endpoint: Some("198.51.100.7:40000".parse().unwrap()),
        handshake_at: handshake.map(Timestamp::from_unix_secs),
        rx_bytes: 10,
        tx_bytes: 20,
    }
}

#[test]
fn peers_are_matched_by_key_and_timed_on_the_controller_s_clock() {
    let mut facts = NodeFacts {
        // The node's clock runs 30 s behind.
        collected_at: Timestamp::from_unix_secs(1_000_030),
        ..NodeFacts::default()
    };
    facts.system.node_time = Some(Timestamp::from_unix_secs(1_000_000));
    facts.services.wireguard = Some(WireguardFacts {
        node_time: Some(Timestamp::from_unix_secs(999_990)),
        public_key: Some("b2xk".to_owned()),
        port: Some(51820),
        peers: vec![
            seen("cGhvbmU=", Some(999_900)),
            seen("bGFwdG9w", Some(999_990 - 181)),
            seen("c3RyYXk=", Some(999_990)),
        ],
    });
    let status = status(Some(&server()), Some(&facts), true);
    assert!(status.online && status.running);
    // The gateway still runs the key it had before the revision made a new one.
    assert!(!status.key_applied);
    let [phone, laptop, tablet] = &status.peers[..] else {
        panic!("{:?}", status.peers)
    };
    assert!(phone.on_gateway && phone.connected);
    assert_eq!(phone.handshake_at, Some(Timestamp::from_unix_secs(999_930)));
    assert_eq!(phone.endpoint.as_deref(), Some("198.51.100.7:40000"));
    assert_eq!((phone.rx_bytes, phone.tx_bytes), (10, 20));
    // Past three minutes, the session is gone.
    assert!(laptop.on_gateway && !laptop.connected);
    // Not applied yet: nothing to show.
    assert!(!tablet.on_gateway && !tablet.connected);
    assert_eq!((tablet.handshake_at, tablet.rx_bytes), (None, 0));
    assert_eq!(
        status.collected_at,
        Some(Timestamp::from_unix_secs(1_000_030))
    );

    facts.services.wireguard.as_mut().unwrap().public_key = Some("c2VydmVy".to_owned());
    assert!(super::status(Some(&server()), Some(&facts), true).key_applied);
}

#[test]
fn without_a_tunnel_or_a_gateway_peers_are_listed_unseen() {
    let status = status(Some(&server()), None, false);
    assert!(!status.online && !status.running && !status.key_applied);
    assert_eq!(status.collected_at, None);
    assert!(status.peers.iter().all(|peer| !peer.on_gateway));
    assert_eq!(status.peers.len(), 3);
    let status = super::status(None, Some(&NodeFacts::default()), true);
    assert!(status.peers.is_empty() && !status.running);
}
