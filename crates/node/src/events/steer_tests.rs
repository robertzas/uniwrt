//! usteer's notifications and hostapd's transition answers, shaped as usteer 2022.08's
//! `usteer_event_send_ubus` and hostapd's `hostapd_ubus_notify_bss_transition_response` build
//! them.

use std::sync::Arc;

use serde_json::value::RawValue;
use uniwrt_model::event::{NodeEvent, SteerAction, SteerEvent, SteerReason, SteerTarget};

use super::{Push, parse};
use crate::proto::AgentEvent;

fn steer(topic: &str, data: &str) -> Option<SteerEvent> {
    let event = AgentEvent::Event {
        topic: topic.to_owned(),
        ts: None,
        data: Arc::from(RawValue::from_string(data.to_owned()).unwrap()),
    };
    match parse(&event).unwrap() {
        Some(Push::Event(NodeEvent::Steer(steer))) => Some(steer),
        None => None,
        Some(other) => panic!("not a steer: {other:?}"),
    }
}

#[test]
fn refusals_name_the_reason_and_the_better_access_point() {
    let low = steer(
        "usteer.assoc_req_deny",
        r#"{"node":"hostapd.phy0-ap0","sta":"02:00:00:00:04:00","signal":-88,
            "reason":"low_signal","threshold":[-88,-85]}"#,
    )
    .unwrap();
    assert_eq!(
        (low.mac.unwrap().to_string(), low.iface.as_str(), low.action),
        (
            "02:00:00:00:04:00".to_owned(),
            "phy0-ap0",
            SteerAction::AssocDenied
        )
    );
    assert_eq!(
        (low.reason, low.signal_dbm, low.min_signal_dbm, low.target),
        (Some(SteerReason::LowSignal), Some(-88), Some(-85), None)
    );
    let better = steer(
        "usteer.probe_req_deny",
        r#"{"node":"hostapd.phy0-ap0","sta":"02:00:00:00:04:00","signal":-70,
            "reason":"better_candidate","select_reason":["signal"],"local":{"load":3,"assoc":1},
            "remote":{"name":"192.168.1.2#hostapd.phy1-ap0","signal":-52,"load":0,"assoc":0}}"#,
    )
    .unwrap();
    assert_eq!(better.min_signal_dbm, None);
    assert_eq!(
        better.target,
        Some(SteerTarget::Usteer {
            host: Some("192.168.1.2".parse().unwrap()),
            iface: "phy1-ap0".to_owned(),
            signal_dbm: Some(-52),
        })
    );
    // Another band of the same node; counts, not signals, for repeated refusals.
    let again = steer(
        "usteer.assoc_req_deny",
        r#"{"node":"hostapd.phy0-ap0","sta":"02:00:00:00:04:00","signal":4294967226,
            "reason":"retry_exceeded","threshold":[5,5],
            "remote":{"name":"hostapd.phy1-ap0"}}"#,
    )
    .unwrap();
    assert_eq!(
        (again.reason, again.signal_dbm, again.min_signal_dbm),
        (Some(SteerReason::RetryExceeded), Some(-70), None)
    );
    assert!(matches!(
        again.target,
        Some(SteerTarget::Usteer { host: None, ref iface, .. }) if iface == "phy1-ap0"
    ));
}

#[test]
fn as_the_agent_pushed_them_in_the_hwsim_lab() {
    // Node a (25.12.6, usteer 2025.10.04~1d6524c6): a signal kick of client c under a
    // minimum of -1 dBm, then c turning a transition request down (status 7, read as `true`).
    let kick = steer(
        "usteer.signal_kick",
        r#"{ "node": "hostapd.phy49-ap0", "threshold": [ -20, -1 ] }"#,
    )
    .unwrap();
    assert_eq!(
        (kick.iface.as_str(), kick.signal_dbm, kick.min_signal_dbm),
        ("phy49-ap0", Some(-20), Some(-1))
    );
    let declined = steer(
        "hostapd.bss-transition-response",
        r#"{ "address": "02:00:00:00:04:00", "dialog-token": true, "status-code": true,
             "bss-termination-delay": false, "ifname": "phy49-ap0" }"#,
    )
    .unwrap();
    assert_eq!(
        (declined.action, declined.status, declined.target),
        (SteerAction::TransitionRejected, None, None)
    );
}

#[test]
fn kicks_and_transition_answers() {
    // usteer names no client in a signal kick: its signal is the threshold's first value.
    let kick = steer(
        "usteer.signal_kick",
        r#"{"node":"hostapd.phy1-ap0","threshold":[-91,-85],"count":1}"#,
    )
    .unwrap();
    assert_eq!(
        (kick.mac, kick.action, kick.signal_dbm, kick.min_signal_dbm),
        (None, SteerAction::SignalKick, Some(-91), Some(-85))
    );
    let load = steer(
        "usteer.load_kick_client",
        r#"{"node":"hostapd.phy1-ap0","sta":"02:00:00:00:04:00","signal":-60,"count":2}"#,
    )
    .unwrap();
    assert_eq!(
        (load.action, load.signal_dbm),
        (SteerAction::LoadKick, Some(-60))
    );
    let accepted = steer(
        "hostapd.bss-transition-response",
        r#"{"address":"02:00:00:00:04:00","dialog-token":1,"status-code":0,
            "bss-termination-delay":0,"target-bssid":"02:00:00:00:01:00","ifname":"phy1-ap0"}"#,
    )
    .unwrap();
    assert_eq!(
        (accepted.action, accepted.status, accepted.target),
        (
            SteerAction::TransitionAccepted,
            Some(0),
            Some(SteerTarget::Bssid {
                bssid: "02:00:00:00:01:00".parse().unwrap()
            })
        )
    );
    let rejected = steer(
        "hostapd.bss-transition-response",
        r#"{"address":"02:00:00:00:04:00","dialog-token":0,"status-code":7,
            "bss-termination-delay":0,"ifname":"phy0-ap0"}"#,
    )
    .unwrap();
    assert_eq!(
        (rejected.action, rejected.status, rejected.target),
        (SteerAction::TransitionRejected, Some(7), None)
    );
    // The load kick's steps that concern no client are not modelled.
    assert!(steer("usteer.load_kick_trigger", r#"{"node":"hostapd.phy1-ap0"}"#).is_none());
}
