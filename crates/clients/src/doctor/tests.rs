use std::{net::IpAddr, time::Duration};

use uniwrt_core::{EventId, LogId, MacAddr, NodeId, Timestamp};
use uniwrt_model::{
    client::{ClientMedium, ClientWifi},
    dns_queries::{DnsQuery, Outcome},
    event::SteerAction,
    events::{EventKind, EventRecord, NewEvent},
    facts::StationCapability,
    log::{LogEntry, LogRecord, LogVia, NewLog, Severity as LogSeverity},
    wifi::Band,
};

use super::{
    DhcpFailure, DhcpStep, Diagnosis, DnsInput, Fix, Input, IssueSeverity, Problem, TIMELINE_MAX,
    TimelineEntry, WifiStep, WlanInfo, diagnose,
};
use crate::{Client, ClientKind, ClientSample, Connection};

const MAC: &str = "aa:bb:cc:00:00:01";
const NOW_MIN: u64 = 1_000;

fn mac() -> MacAddr {
    MAC.parse().unwrap()
}

/// `minute` minutes after the epoch; now is minute 1,000, and the span starts at minute 100.
fn t(minute: u64) -> Timestamp {
    Timestamp::from_unix_ms(i64::try_from(minute * 60_000).unwrap())
}

fn client(band: Band, since: u64) -> Client {
    Client {
        mac: mac(),
        alias: None,
        hostname: Some("phone".to_owned()),
        vendor: None,
        kind: ClientKind::Unknown,
        randomized: false,
        group: None,
        ips: vec!["192.168.1.20".parse().unwrap()],
        online: true,
        connection: Some(Connection {
            node: Some(NodeId::new(2)),
            upstream: "node:2".to_owned(),
            upstream_name: None,
            port: Some("phy1-ap0".to_owned()),
            medium: ClientMedium::Wifi,
            since: t(since),
            band: Some(band),
            ssid: Some("home".to_owned()),
            channel: Some(36),
            signal_dbm: Some(-60),
            noise_dbm: None,
            rx_rate: None,
            tx_rate: None,
            standard: None,
            capabilities: Vec::new(),
            capacity_mbps: None,
            traffic: None,
        }),
        first_seen: t(0),
        last_seen: t(NOW_MIN),
        blocked: None,
        experience: None,
        wifi: ClientWifi::default(),
    }
}

fn samples(dbm: i16) -> Vec<ClientSample> {
    (0..10)
        .map(|i| ClientSample {
            at: t(NOW_MIN - 10 + i),
            signal_dbm: Some(dbm),
            snr_db: None,
            tx_kbps: None,
            rx_kbps: None,
            down_bps: None,
            up_bps: None,
            score: None,
        })
        .collect()
}

fn wlan(access_points: usize, steering: bool, fast_roaming: bool) -> WlanInfo {
    WlanInfo {
        ssid: "home".to_owned(),
        fast_roaming,
        steering,
        bands: vec![Band::G2, Band::G5],
        access_points,
    }
}

fn event(id: i64, minute: u64, what: EventKind) -> EventRecord {
    EventRecord::new(
        EventId::new(id),
        NewEvent::new(t(minute), what).client(mac()),
    )
}

fn log(id: i64, minute: u64, program: &str, message: &str) -> LogRecord {
    LogRecord::new(
        LogId::new(id),
        NewLog {
            received: t(minute),
            node: Some(NodeId::new(1)),
            host: None,
            via: LogVia::Agent,
            entry: LogEntry {
                seq: None,
                time: t(minute),
                facility: 3,
                severity: LogSeverity::Info,
                program: Some(program.to_owned()),
                pid: Some(1),
                message: message.to_owned(),
            },
            pattern: None,
        },
    )
}

fn failed_query(minute: u64, domain: &str) -> DnsQuery {
    DnsQuery {
        at: t(minute),
        client: "192.168.1.20".parse::<IpAddr>().unwrap(),
        mac: Some(mac()),
        qtype: "A".to_owned(),
        domain: domain.to_owned(),
        outcome: Outcome::Failed,
    }
}

struct Case {
    client: Client,
    samples: Vec<ClientSample>,
    events: Vec<EventRecord>,
    logs: Vec<LogRecord>,
    dns: DnsInput,
    wlans: Vec<WlanInfo>,
}

impl Case {
    fn new(client: Client) -> Self {
        Self {
            client,
            samples: samples(-55),
            events: Vec::new(),
            logs: Vec::new(),
            dns: DnsInput::default(),
            wlans: vec![wlan(1, true, false)],
        }
    }

    fn run(&self) -> Diagnosis {
        diagnose(&Input {
            now: t(NOW_MIN),
            from: t(100),
            client: &self.client,
            samples: &self.samples,
            events: &self.events,
            logs: &self.logs,
            dns: &self.dns,
            wlans: &self.wlans,
        })
    }
}

fn problems(diagnosis: &Diagnosis) -> Vec<&Problem> {
    diagnosis
        .issues
        .iter()
        .map(|issue| &issue.problem)
        .collect()
}

#[test]
fn a_healthy_client_has_no_issues() {
    let diagnosis = Case::new(client(Band::G5, 900)).run();
    assert!(diagnosis.issues.is_empty(), "{:?}", diagnosis.issues);
    assert_eq!(diagnosis.timeline, []);
}

#[test]
fn a_weak_signal_is_a_warning_and_a_very_weak_one_critical() {
    let mut case = Case::new(client(Band::G5, 990));
    case.samples = samples(-78);
    let diagnosis = case.run();
    assert_eq!(
        problems(&diagnosis),
        [&Problem::WeakSignal {
            signal_dbm: -78,
            node: Some(NodeId::new(2)),
            band: Some(Band::G5),
        }]
    );
    assert_eq!(diagnosis.issues[0].severity, IssueSeverity::Warning);
    assert_eq!(diagnosis.issues[0].fixes[1], Fix::AddAccessPoint);
    case.samples = samples(-85);
    assert_eq!(case.run().issues[0].severity, IssueSeverity::Critical);
    // Too few recent samples say nothing.
    case.samples.truncate(2);
    assert_eq!(case.run().issues, []);
}

#[test]
fn a_weak_client_that_stays_where_others_carry_its_wlan_is_sticky() {
    let mut case = Case::new(client(Band::G5, 950));
    case.samples = samples(-79);
    case.wlans = vec![wlan(3, false, false)];
    let declined = EventKind::ClientSteered {
        action: SteerAction::TransitionRejected,
        iface: "phy1-ap0".to_owned(),
        reason: None,
        signal_dbm: Some(-79),
        min_signal_dbm: None,
        to: None,
        to_signal_dbm: None,
        status: Some(6),
    };
    case.events = vec![event(1, 960, declined.clone()), event(2, 970, declined)];
    let diagnosis = case.run();
    assert_eq!(
        problems(&diagnosis),
        [&Problem::Sticky {
            signal_dbm: -79,
            node: Some(NodeId::new(2)),
            minutes: 50,
            declined: 2,
        }]
    );
    assert_eq!(
        diagnosis.issues[0].fixes[0],
        Fix::TurnOnSteering {
            ssid: "home".to_owned()
        }
    );
}

#[test]
fn repeated_handshake_failures_point_at_the_passphrase() {
    let mut case = Case::new(client(Band::G5, 900));
    case.client.online = false;
    case.logs = (0..3)
        .map(|i| {
            log(
                i,
                950 + u64::try_from(i).unwrap(),
                "hostapd",
                &format!("phy1-ap0: AP-STA-POSSIBLE-PSK-MISMATCH {MAC}"),
            )
        })
        .collect();
    let diagnosis = case.run();
    assert_eq!(diagnosis.issues[0].severity, IssueSeverity::Critical);
    assert!(matches!(
        diagnosis.issues[0].problem,
        Problem::AuthFailures {
            failures: 3,
            connected: false,
            ..
        }
    ));
    assert!(matches!(
        &diagnosis.timeline[0],
        TimelineEntry::Wifi { step: WifiStep::AuthFailed, interface: Some(i), .. } if i == "phy1-ap0"
    ));
    // Getting on afterwards makes it a warning.
    case.events = vec![event(
        1,
        960,
        EventKind::ClientJoined {
            medium: ClientMedium::Wifi,
            place: None,
            ssid: Some("home".to_owned()),
            band: Some(Band::G5),
        },
    )];
    assert_eq!(case.run().issues[0].severity, IssueSeverity::Warning);
}

fn dhcp_log(id: i64, minute: u64, line: &str) -> LogRecord {
    log(id, minute, "dnsmasq-dhcp", &line.replace("MAC", MAC))
}

#[test]
fn a_refusal_followed_by_an_address_is_no_issue() {
    let mut case = Case::new(client(Band::G5, 900));
    case.logs = vec![
        dhcp_log(1, 900, "DHCPREQUEST(br-lan) 10.0.0.5 MAC"),
        dhcp_log(2, 900, "DHCPNAK(br-lan) 10.0.0.5 MAC wrong network"),
        dhcp_log(3, 900, "DHCPDISCOVER(br-lan) MAC"),
        dhcp_log(4, 900, "DHCPOFFER(br-lan) 192.168.1.20 MAC"),
        dhcp_log(5, 900, "123 DHCPACK(br-lan) 192.168.1.20 MAC phone"),
    ];
    let diagnosis = case.run();
    assert!(diagnosis.issues.is_empty(), "{:?}", diagnosis.issues);
    assert_eq!(diagnosis.timeline.len(), 5);
    assert!(diagnosis.timeline.iter().any(|entry| matches!(
        entry,
        TimelineEntry::Dhcp { step: DhcpStep::Ack, ip: Some(ip), detail: Some(host), .. }
            if ip == "192.168.1.20" && host == "phone"
    )));
}

#[test]
fn a_full_pool_and_an_unanswered_ask_are_dhcp_failures() {
    let mut case = Case::new(client(Band::G5, 900));
    case.logs = vec![
        dhcp_log(1, 990, "DHCPDISCOVER(br-lan) MAC no address available"),
        dhcp_log(2, 991, "DHCPDISCOVER(br-lan) MAC no address available"),
    ];
    let diagnosis = case.run();
    assert_eq!(
        problems(&diagnosis),
        [&Problem::DhcpFailures {
            failures: 2,
            reason: DhcpFailure::PoolFull,
        }]
    );
    assert_eq!(diagnosis.issues[0].severity, IssueSeverity::Critical);
    assert_eq!(diagnosis.issues[0].fixes, [Fix::WidenDhcpPool]);

    case.logs = vec![dhcp_log(1, 995, "DHCPDISCOVER(br-lan) MAC")];
    assert!(matches!(
        problems(&case.run())[..],
        [Problem::DhcpFailures {
            failures: 1,
            reason: DhcpFailure::NoAnswer,
        }]
    ));
    // An ask still within its minute is no failure yet.
    case.logs = vec![dhcp_log(1, NOW_MIN, "DHCPDISCOVER(br-lan) MAC")];
    assert_eq!(case.run().issues, []);
}

#[test]
fn a_client_online_without_an_address_is_flagged() {
    let mut case = Case::new(client(Band::G5, 990));
    case.client.ips.clear();
    assert_eq!(problems(&case.run()), [&Problem::NoAddress { minutes: 10 }]);
}

#[test]
fn failed_and_blocked_dns_queries_are_weighed_against_all_of_them() {
    let mut case = Case::new(client(Band::G5, 900));
    case.dns = DnsInput {
        logged: true,
        queries: 100,
        blocked: 60,
        failed: 35,
        failures: vec![
            failed_query(990, "a.example"),
            failed_query(988, "b.example"),
            failed_query(700, "a.example"),
        ],
    };
    let diagnosis = case.run();
    assert_eq!(
        problems(&diagnosis),
        [
            &Problem::DnsFailures {
                failed: 35,
                queries: 100
            },
            &Problem::DnsBlocked {
                blocked: 60,
                queries: 100
            },
        ]
    );
    assert_eq!(diagnosis.issues[0].severity, IssueSeverity::Critical);
    assert_eq!(diagnosis.issues[0].at, t(990));
    // Two groups: 988–990, and 700 alone.
    let groups: Vec<_> = diagnosis
        .timeline
        .iter()
        .map(|entry| match entry {
            TimelineEntry::Dns {
                at,
                failed,
                domains,
            } => (*at, *failed, domains.len()),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(groups, [(t(988), 2, 2), (t(700), 1, 1)]);
    // Without the query log, nothing is said.
    case.dns.logged = false;
    assert_eq!(case.run().issues, []);
}

#[test]
fn a_dual_band_client_on_2g_with_a_strong_signal_is_stuck() {
    let mut case = Case::new(client(Band::G2, 900));
    case.client.wifi.bands = [Band::G2, Band::G5].into();
    case.wlans = vec![wlan(1, false, false)];
    let diagnosis = case.run();
    assert!(matches!(
        problems(&diagnosis)[..],
        [Problem::StuckOn2g {
            signal_dbm: -55,
            ..
        }]
    ));
    assert_eq!(diagnosis.issues[0].severity, IssueSeverity::Info);
    case.client.wifi.bands = [Band::G2].into();
    assert_eq!(case.run().issues, []);
}

#[test]
fn short_sessions_on_an_802_11r_wlan_suggest_turning_it_off() {
    let mut case = Case::new(client(Band::G5, 900));
    case.wlans = vec![wlan(2, true, true)];
    case.events = (0..3)
        .map(|i| {
            event(
                i,
                800 + u64::try_from(i).unwrap() * 30,
                EventKind::ClientLeft {
                    connected_ms: Some(20_000),
                },
            )
        })
        .collect();
    let diagnosis = case.run();
    assert_eq!(
        problems(&diagnosis),
        [&Problem::FastRoaming {
            ssid: "home".to_owned(),
            short_sessions: 3,
            failures: 0,
        }]
    );
    assert_eq!(
        diagnosis.issues[0].fixes[0],
        Fix::TurnOffFastRoaming {
            ssid: "home".to_owned()
        }
    );
    // A client that fast-roamed handles 802.11r.
    case.client.wifi.capabilities = [StationCapability::FastTransition].into();
    assert_eq!(case.run().issues, []);
}

#[test]
fn many_disconnects_in_an_hour_are_flapping() {
    let mut case = Case::new(client(Band::G5, 999));
    case.events = (0..6)
        .map(|i| {
            event(
                i,
                950 + u64::try_from(i).unwrap() * 5,
                EventKind::ClientLeft {
                    connected_ms: Some(300_000),
                },
            )
        })
        .collect();
    assert_eq!(
        problems(&case.run()),
        [&Problem::Flapping { disconnects: 6 }]
    );
    // Older ones don't count.
    case.events[0].at = t(900);
    assert_eq!(case.run().issues, []);
}

#[test]
fn the_timeline_is_newest_first_without_routine_lines_or_other_clients() {
    let mut case = Case::new(client(Band::G5, 900));
    case.logs = vec![
        log(
            1,
            910,
            "hostapd",
            &format!("phy1-ap0: AP-STA-CONNECTED {MAC}"),
        ),
        log(
            2,
            911,
            "hostapd",
            &format!("phy1-ap0: STA {MAC} IEEE 802.11: disassociated due to inactivity"),
        ),
        log(3, 912, "kernel", &format!("something about {MAC}")),
        // A full-text match on the parts of the address, not the address.
        log(4, 913, "kernel", "aa bb cc 00 00 01"),
        log(
            5,
            914,
            "hostapd",
            &format!("phy1-ap0: STA {MAC} FT: invalid MIC in FTIE"),
        ),
        // Out of the span.
        log(6, 10, "kernel", &format!("old {MAC}")),
    ];
    case.events = vec![event(1, 905, EventKind::ClientLeft { connected_ms: None })];
    let diagnosis = case.run();
    let ats: Vec<_> = diagnosis.timeline.iter().map(TimelineEntry::at).collect();
    assert_eq!(ats, [t(914), t(912), t(911), t(905)]);
    assert!(matches!(
        diagnosis.timeline[0],
        TimelineEntry::Wifi {
            step: WifiStep::FastTransitionFailed,
            ..
        }
    ));
    assert!(matches!(
        diagnosis.timeline[2],
        TimelineEntry::Wifi {
            step: WifiStep::Dropped,
            ..
        }
    ));
    assert!(
        matches!(&diagnosis.timeline[1], TimelineEntry::Log { program: Some(p), .. } if p == "kernel")
    );
    assert!(!diagnosis.truncated);
}

#[test]
fn a_long_timeline_keeps_the_newest_entries() {
    let mut case = Case::new(client(Band::G5, 900));
    let count = i64::try_from(TIMELINE_MAX).unwrap() + 10;
    case.events = (0..count)
        .map(|i| {
            let mut record = event(i, 0, EventKind::ClientLeft { connected_ms: None });
            record.at =
                t(NOW_MIN - 100).saturating_add(Duration::from_secs(u64::try_from(i).unwrap()));
            record.last_at = record.at;
            record
        })
        .collect();
    let diagnosis = case.run();
    assert!(diagnosis.truncated);
    assert_eq!(diagnosis.timeline.len(), TIMELINE_MAX);
    assert_eq!(diagnosis.timeline[0].at(), case.events.last().unwrap().at);
}

#[test]
fn a_diagnosis_serializes_with_tagged_kinds() {
    let mut case = Case::new(client(Band::G5, 990));
    case.samples = samples(-78);
    let value = serde_json::to_value(case.run()).unwrap();
    assert_eq!(value["issues"][0]["problem"]["kind"], "weak_signal");
    assert_eq!(value["issues"][0]["fixes"][0]["fix"], "move_closer");
    assert_eq!(value["issues"][0]["severity"], "warning");
}
