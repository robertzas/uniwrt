use serde_json::json;
use uniwrt_core::{NodeId, Timestamp};
use uniwrt_model::{
    event::{Attachment, SteerAction, SteerEvent, SteerReason, SteerTarget},
    events::{EventKind, EventLevel},
    facts::NodeFacts,
    node::NodeRole,
};

use crate::{DeriveConfig, Deriver, Seed};

const A: NodeId = NodeId::new(1);
const B: NodeId = NodeId::new(2);
const PHONE: &str = "f0:18:98:00:00:01";

const fn at(s: i64) -> Timestamp {
    Timestamp::from_unix_ms(s * 1_000)
}

/// A node at `address` on the LAN with a 5 GHz access point `phy1-ap0` of `bssid`.
fn facts(address: &str, bssid: &str) -> NodeFacts {
    NodeFacts {
        interfaces: serde_json::from_value(json!([{
            "name": "lan", "proto": "static", "up": true, "device": "br-lan", "uptime_s": 10,
            "ipv4": [format!("{address}/24")], "ipv6": [], "delegated_prefixes": [],
            "ipv4_gateway": null, "ipv6_gateway": null, "dns_servers": [], "errors": []
        }]))
        .unwrap(),
        radios: vec![
            serde_json::from_value(json!({
                "name": "radio1", "band": "5g", "up": true, "disabled": false, "channel": 36,
                "ifaces": [{"name": "phy1-ap0", "section": "w0", "mode": "ap", "ssid": "Home",
                            "mesh_id": null, "bssid": bssid, "encryption": "sae",
                            "networks": ["lan"], "up": true}]
            }))
            .unwrap(),
        ],
        ..NodeFacts::default()
    }
}

fn deriver() -> Deriver {
    let mut d = Deriver::new(DeriveConfig::default(), Seed::default());
    d.facts(
        A,
        NodeRole::Gateway,
        &facts("192.168.1.1", "02:00:00:00:0a:01"),
        false,
        at(0),
    );
    d.facts(
        B,
        NodeRole::Ap,
        &facts("192.168.1.2", "02:00:00:00:0b:01"),
        false,
        at(0),
    );
    d
}

fn steer(action: SteerAction, target: Option<SteerTarget>) -> SteerEvent {
    SteerEvent {
        mac: Some(PHONE.parse().unwrap()),
        iface: "phy0-ap0".to_owned(),
        action,
        reason: Some(SteerReason::BetterCandidate),
        signal_dbm: Some(-71),
        min_signal_dbm: None,
        target,
        status: None,
    }
}

fn to(event: &EventKind) -> Option<Attachment> {
    match event {
        EventKind::ClientSteered { to, .. } => to.clone(),
        other => panic!("not steered: {other:?}"),
    }
}

#[test]
fn the_access_point_steered_to_is_placed_by_address_or_bssid() {
    let d = deriver();
    let usteer = |host: Option<&str>| SteerTarget::Usteer {
        host: host.map(|host| host.parse().unwrap()),
        iface: "phy1-ap0".to_owned(),
        signal_dbm: Some(-50),
    };
    let place = |node, iface: &str| {
        Some(Attachment {
            node,
            iface: iface.to_owned(),
        })
    };
    let refused = d.steered(
        A,
        &steer(SteerAction::AssocDenied, Some(usteer(Some("192.168.1.2")))),
        at(5),
    );
    assert_eq!(to(&refused.what), place(B, "phy1-ap0"));
    assert_eq!(
        (refused.node, refused.client),
        (Some(A), Some(PHONE.parse().unwrap()))
    );
    assert_eq!(refused.level, EventLevel::Info);
    // Another band of the same node; a usteer the controller doesn't know.
    let local = d.steered(
        A,
        &steer(SteerAction::ProbeDenied, Some(usteer(None))),
        at(5),
    );
    assert_eq!(to(&local.what), place(A, "phy1-ap0"));
    let stranger = d.steered(
        A,
        &steer(SteerAction::AssocDenied, Some(usteer(Some("10.0.0.9")))),
        at(5),
    );
    assert_eq!(to(&stranger.what), None);
    let answer = SteerTarget::Bssid {
        bssid: "02:00:00:00:0b:01".parse().unwrap(),
    };
    let accepted = d.steered(
        A,
        &steer(SteerAction::TransitionAccepted, Some(answer)),
        at(6),
    );
    assert_eq!(to(&accepted.what), place(B, "phy1-ap0"));
    assert_eq!(accepted.fold, None);
}

#[test]
fn refusals_fold_and_kicks_matter_more() {
    let d = deriver();
    let probe = d.steered(A, &steer(SteerAction::ProbeDenied, None), at(1));
    let assoc = d.steered(A, &steer(SteerAction::AssocDenied, None), at(1));
    assert_eq!(
        probe.fold.as_deref(),
        Some("steer/1/f0:18:98:00:00:01/phy0-ap0/probe")
    );
    assert_ne!(probe.fold, assoc.fold);
    let mut kick = steer(SteerAction::SignalKick, None);
    kick.mac = None;
    let kicked = d.steered(A, &kick, at(2));
    assert_eq!(
        (kicked.client, kicked.fold, kicked.level),
        (None, None, EventLevel::Notice)
    );
}
