use std::sync::Arc;

use serde_json::value::RawValue;
use uniwrt_core::{MacAddr, Timestamp};
use uniwrt_model::{
    event::{
        DeviceChange::{Added, Down, LinkDown, LinkUp, Removed, Up},
        DeviceEvent, InterfaceEvent, LeaseChange, NodeEvent,
        StationChange::{AssocRequest, AuthRequest, Authorized, Disassociated},
        StationEvent,
    },
    log::{LogEntry, Severity},
};

use super::{Push, parse};
use crate::proto::{AgentEvent, Message, ops::Set};

/// What node a's agent pushed in the hwsim lab (`scripts/wifi-lab.sh capture events`).
const CAPTURE: &str = include_str!("../../fixtures/events/hwsim-a-25.12.6.jsonl");
const CLIENT: &str = "02:00:00:00:04:00";

fn mac(text: &str) -> MacAddr {
    text.parse().unwrap()
}

fn pushes() -> Vec<Push> {
    CAPTURE
        .lines()
        .filter_map(|line| match Message::parse(line.as_bytes()).unwrap() {
            Message::Event(event) => Some(event),
            _ => None,
        })
        .map(|event| {
            parse(&event)
                .unwrap()
                .expect("every captured push is modeled")
        })
        .collect()
}

fn event(topic: &str, data: &str) -> AgentEvent {
    AgentEvent::Event {
        topic: topic.to_owned(),
        ts: None,
        data: Arc::from(RawValue::from_string(data.to_owned()).unwrap()),
    }
}

#[test]
fn a_kicked_client_leaves_comes_back_and_renews_its_lease() {
    let pushes = pushes();
    let client: Vec<_> = pushes
        .iter()
        .filter_map(|push| match push {
            Push::Event(NodeEvent::Station(station)) => Some(station),
            _ => None,
        })
        .collect();
    assert!(
        client
            .iter()
            .all(|s| s.mac == mac(CLIENT) && s.iface == "phy1-ap0")
    );
    let mut changes: Vec<_> = client.iter().map(|s| s.change).collect();
    // Kicked, it comes back at once. When radio0 restarts, it retries authenticating a few
    // times while the AP comes up.
    changes.dedup();
    assert_eq!(
        changes,
        [
            Disassociated,
            AuthRequest,
            AssocRequest,
            Authorized,
            Disassociated,
            AuthRequest,
            AssocRequest,
            Authorized
        ]
    );
    assert_eq!(
        *client[2],
        StationEvent {
            mac: mac(CLIENT),
            iface: "phy1-ap0".to_owned(),
            change: AssocRequest,
            signal_dbm: Some(-20),
            freq_mhz: Some(2412),
            auth_alg: None,
        }
    );
    assert_eq!(client[3].auth_alg.as_deref(), Some("open"));
    let leases: Vec<_> = pushes
        .iter()
        .filter_map(|push| match push {
            Push::Event(NodeEvent::Lease(lease)) => Some(lease),
            _ => None,
        })
        .collect();
    assert_eq!(leases.len(), 2);
    assert!(leases.iter().all(|lease| lease.change == LeaseChange::Acked
        && lease.mac == Some(mac(CLIENT))
        && lease.hostname.as_deref() == Some("lab-client")
        && lease.device.as_deref() == Some("br-mesh")));
}

#[test]
fn interfaces_and_devices_report_their_changes() {
    let pushes = pushes();
    let interfaces: Vec<_> = pushes
        .iter()
        .filter_map(|push| match push {
            Push::Event(NodeEvent::Interface(iface)) => Some(iface),
            _ => None,
        })
        .collect();
    // netifd notifies loopback's going down twice and its coming up twice.
    let ups: Vec<_> = interfaces.iter().map(|iface| iface.up).collect();
    assert_eq!(ups, [false, false, true, true]);
    assert_eq!(
        *interfaces[2],
        InterfaceEvent {
            name: "loopback".to_owned(),
            up: true,
            device: Some("lo".to_owned()),
            updated: vec!["addresses".to_owned()],
        }
    );
    let radio: Vec<_> = pushes
        .iter()
        .filter_map(|push| match push {
            Push::Event(NodeEvent::Device(device)) if device.name == "phy1-ap0" => {
                Some(device.change)
            }
            _ => None,
        })
        .collect();
    assert_eq!(radio, [Down, Removed, LinkDown, Removed, Up, Added, LinkUp]);
    let link_up = pushes.iter().find_map(|push| match push {
        Push::Event(NodeEvent::Device(device))
            if device.name == "phy1-ap0" && device.change == LinkUp =>
        {
            Some(device)
        }
        _ => None,
    });
    assert_eq!(
        link_up,
        Some(&DeviceEvent {
            name: "phy1-ap0".to_owned(),
            change: LinkUp,
            present: true,
            active: true,
            carrier: true,
        })
    );
}

#[test]
fn polled_and_streamed_logs_parse_alike() {
    let pushes = pushes();
    let batches: Vec<_> = pushes
        .iter()
        .filter_map(|push| match push {
            Push::Log { entries, dropped } => Some((entries, *dropped)),
            _ => None,
        })
        .collect();
    assert!(
        batches
            .iter()
            .all(|&(entries, dropped)| !entries.is_empty() && dropped == 0)
    );
    let (polled, streamed): (Vec<&LogEntry>, Vec<_>) = batches
        .iter()
        .flat_map(|(entries, _)| entries.iter())
        .partition(|entry| entry.seq.is_some());
    // Polls send logd's entries in order, without gaps.
    let seqs: Vec<_> = polled.iter().filter_map(|entry| entry.seq).collect();
    assert!(seqs.windows(2).all(|w| w[1] == w[0] + 1), "{seqs:?}");
    let disconnected = polled
        .iter()
        .find(|entry| entry.message.contains("AP-STA-DISCONNECTED"))
        .unwrap();
    assert_eq!(
        (disconnected.program.as_deref(), disconnected.pid),
        (Some("hostapd"), None)
    );
    assert_eq!(
        (disconnected.facility, disconnected.severity),
        (3, Severity::Notice)
    );
    assert_eq!(
        disconnected.message,
        format!("phy1-ap0: AP-STA-DISCONNECTED {CLIENT}")
    );
    let restarted = streamed.last().unwrap();
    assert_eq!(
        **restarted,
        LogEntry {
            seq: None,
            time: Timestamp::from_unix_ms(1_793_245_545_037),
            facility: 1,
            severity: Severity::Notice,
            program: Some("lab".to_owned()),
            pid: None,
            message: "loopback restarted".to_owned(),
        }
    );
    assert!(
        streamed
            .iter()
            .any(|entry| entry.program.as_deref() == Some("netifd"))
    );
}

#[test]
fn ticks_are_collect_answers() {
    let tick = CAPTURE
        .lines()
        .find_map(|line| match Message::parse(line.as_bytes()).unwrap() {
            Message::Event(tick @ AgentEvent::Tick(_)) => Some(tick),
            _ => None,
        })
        .unwrap();
    let Some(Push::Tick(collected)) = parse(&tick).unwrap() else {
        panic!("a tick");
    };
    assert!(collected.sets.contains_key(&Set::Leases) && collected.errors.is_empty());
}

#[test]
fn kernel_lines_untagged_messages_and_garbage() {
    let polled = event(
        "log",
        r#"{"entries":[
            {"id":7,"priority":4,"source":0,"time":1000,"msg":"[   12.5] eth0: link up"},
            {"id":8,"priority":14,"source":1,"time":1001,"msg":"just text: no tag"},
            {"id":9,"priority":30,"source":1,"time":1002,"msg":"dnsmasq-dhcp[2092]: DHCPACK(br-lan)"}
        ],"dropped":3}"#,
    );
    let Some(Push::Log { entries, dropped }) = parse(&polled).unwrap() else {
        panic!("a log batch");
    };
    assert_eq!(dropped, 3);
    let tags: Vec<_> = entries
        .iter()
        .map(|e| (e.program.as_deref(), e.pid, e.message.as_str(), e.severity))
        .collect();
    assert_eq!(
        tags,
        [
            (
                Some("kernel"),
                None,
                "[   12.5] eth0: link up",
                Severity::Warning
            ),
            (None, None, "just text: no tag", Severity::Info),
            (
                Some("dnsmasq-dhcp"),
                Some(2092),
                "DHCPACK(br-lan)",
                Severity::Info
            ),
        ]
    );
    let streamed = event(
        "log",
        r#"{"lines":[
            "Thu Oct 29 03:40:30 2026 [1793245230.849] kern.err kernel: [   12.5] mt76: timeout",
            "Thu Oct 29 03:40:30 2026 [1793245230.850] local7.debug app[x]: odd pid",
            "not a log line"
        ]}"#,
    );
    let Some(Push::Log { entries, dropped }) = parse(&streamed).unwrap() else {
        panic!("a log batch");
    };
    assert_eq!(dropped, 1);
    assert_eq!(
        (entries[0].facility, entries[0].program.as_deref()),
        (0, Some("kernel"))
    );
    assert_eq!(entries[0].message, "[   12.5] mt76: timeout");
    assert_eq!(
        (entries[1].facility, entries[1].program.as_deref()),
        (23, None)
    );
}

#[test]
fn releases_unknown_types_and_wrong_shapes() {
    // As dnsmasq sends it: a release names no host.
    let release = event(
        "dnsmasq.dhcp.release",
        r#"{"mac":"02:00:00:00:04:00","ip":"10.99.0.108","interface":"br-mesh"}"#,
    );
    let Some(Push::Event(NodeEvent::Lease(lease))) = parse(&release).unwrap() else {
        panic!("a lease");
    };
    assert_eq!(
        (lease.change, lease.hostname),
        (LeaseChange::Released, None)
    );
    assert!(
        parse(&event("hostapd.beacon-report", r#"{"x":1}"#))
            .unwrap()
            .is_none()
    );
    assert!(parse(&event("later.topic", "{}")).unwrap().is_none());
    let ended = parse(&event("log.end", r#"{"reason":"logread exited"}"#)).unwrap();
    assert!(matches!(ended, Some(Push::LogEnded { reason }) if reason == "logread exited"));
    let err = parse(&event("hostapd.assoc", r#"{"ifname":"phy1-ap0"}"#)).unwrap_err();
    assert_eq!(err.topic, "hostapd.assoc");
    assert!(err.reason.contains("address"), "{err}");
}
