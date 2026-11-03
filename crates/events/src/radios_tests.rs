//! Radio events over hand-made facts: channel changes, with and without radar before them, and
//! a congested channel.

use std::sync::Arc;

use serde_json::json;
use uniwrt_core::{LogId, NodeId, Timestamp};
use uniwrt_model::{
    events::{EventKind, EventLevel, NewEvent},
    facts::NodeFacts,
    log::{LogEntry, LogRecord, LogVia, Severity},
    node::NodeRole,
    wifi::Band,
};

use crate::{DeriveConfig, Deriver, Seed};

const N1: NodeId = NodeId::new(1);

const fn at(s: i64) -> Timestamp {
    Timestamp::from_unix_ms(s * 1_000)
}

fn facts(s: i64, channel: u16, busy: f32) -> NodeFacts {
    let mut facts = NodeFacts {
        collected_at: at(s),
        ..NodeFacts::default()
    };
    facts.radios = vec![
        serde_json::from_value(json!({
            "name": "radio1", "band": "5g", "up": true, "disabled": false,
            "channel": channel, "width": 80, "channel_busy_percent": busy, "ifaces": []
        }))
        .unwrap(),
    ];
    facts
}

fn radar(s: i64) -> LogRecord {
    LogRecord {
        id: LogId::new(s),
        received: at(s),
        node: Some(N1),
        host: None,
        via: LogVia::Agent,
        entry: LogEntry {
            seq: None,
            time: at(s),
            facility: 3,
            severity: Severity::Warning,
            program: Some("hostapd".into()),
            pid: None,
            message: "phy1-ap0: DFS-RADAR-DETECTED freq=5500 chan_width=3 cf1=5530".into(),
        },
        pattern: Some("dfs-radar".into()),
    }
}

fn changes(events: &[NewEvent]) -> Vec<(u16, u16, bool, EventLevel)> {
    events
        .iter()
        .filter_map(|e| match &e.what {
            EventKind::ChannelChanged {
                from,
                to,
                radar,
                band: Band::G5,
                ..
            } => Some((*from, *to, *radar, e.level)),
            _ => None,
        })
        .collect()
}

fn feed(d: &mut Deriver, facts: &NodeFacts) -> Vec<NewEvent> {
    d.facts(N1, NodeRole::Ap, facts, false, facts.collected_at)
}

#[test]
fn a_channel_change_is_an_event_marked_when_radar_came_just_before() {
    let mut d = Deriver::new(DeriveConfig::default(), Seed::default());
    assert_eq!(feed(&mut d, &facts(0, 36, 10.0)).len(), 0);
    let moved = feed(&mut d, &facts(60, 100, 10.0));
    assert_eq!(changes(&moved), [(36, 100, false, EventLevel::Notice)]);
    let logged = d.logged(&[radar(100)], |_| None);
    assert_eq!(logged.len(), 0, "the pattern is not an event here");
    let fled = feed(&mut d, &facts(150, 44, 10.0));
    assert_eq!(changes(&fled), [(100, 44, true, EventLevel::Warning)]);
    let later = feed(&mut d, &facts(400, 36, 10.0));
    assert_eq!(changes(&later), [(44, 36, false, EventLevel::Notice)]);
}

#[test]
fn channels_known_at_start_are_not_changes() {
    let seed = Seed {
        facts: vec![(N1, NodeRole::Ap, Arc::new(facts(0, 36, 10.0)))],
        ..Seed::default()
    };
    let mut d = Deriver::new(DeriveConfig::default(), seed);
    assert_eq!(feed(&mut d, &facts(10, 36, 10.0)).len(), 0);
}

#[test]
fn a_channel_busy_for_fifteen_minutes_is_congested_until_it_quiets() {
    let mut d = Deriver::new(DeriveConfig::default(), Seed::default());
    assert_eq!(feed(&mut d, &facts(0, 36, 85.0)).len(), 0);
    assert_eq!(feed(&mut d, &facts(600, 36, 90.0)).len(), 0);
    let raised = feed(&mut d, &facts(900, 36, 88.0));
    let [event] = raised.as_slice() else {
        panic!("{raised:?}")
    };
    assert_eq!(event.at, at(0), "stamped when it began");
    assert!(matches!(
        event.what,
        EventKind::ChannelCongested {
            busy_percent: 88,
            ..
        }
    ));
    assert_eq!(
        event.condition.as_ref().map(|c| c.key.as_str()),
        Some("node:1/radio:radio1/busy")
    );
    assert_eq!(
        feed(&mut d, &facts(1000, 36, 70.0)).len(),
        0,
        "between the thresholds"
    );
    let cleared = feed(&mut d, &facts(1200, 36, 40.0));
    assert!(matches!(
        cleared.as_slice(),
        [NewEvent {
            what: EventKind::ChannelClear {
                busy_percent: 40,
                congested_ms: Some(1_200_000),
                ..
            },
            ..
        }]
    ));
}
