//! Alerts (features/observability.md, Alerts): a rule firing for one subject (a node, a radio, a
//! client, a link, the site) until it resolves on its own. Users acknowledge alerts, and
//! maintenance windows silence them; silenced alerts are still recorded.

mod rule;

use serde::{Deserialize, Serialize};
use uniwrt_core::{AlertId, EventId, MacAddr, NodeId, RuleId, Timestamp, UserId, WindowId};

pub use self::rule::{
    AlertRule, Comparison, EntityPattern, EventCondition, MAX_FOR_S, MAX_WINDOW_S, MetricCondition,
    ParsePatternError, RuleCondition, RuleSpec,
};
use crate::{events::EventLevel, metric::Metric};

/// What made an alert fire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AlertCause {
    /// A series beyond its threshold: the value it fired at, and the worst since.
    Metric {
        metric: Metric,
        op: Comparison,
        threshold: f64,
        value: f64,
        worst: f64,
    },
    /// Matching events: the latest, how many are in the window, and the condition it raised
    /// (the alert resolves when it clears).
    Event {
        event: EventId,
        kind: String,
        matches: u32,
        condition: Option<String>,
    },
}

/// Why an alert resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum AlertResolution {
    /// Back to normal: the series is on the safe side, the condition cleared, or the window
    /// holds fewer matches.
    Cleared,
    /// The series stopped (its node went offline, its client left).
    NoData,
    /// Its rule was disabled, deleted or changed.
    RuleChanged,
}

/// Who acknowledged an alert, and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Acknowledgement {
    pub by: UserId,
    pub username: String,
    pub at: Timestamp,
}

/// An alert as recorded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct AlertRecord {
    pub id: AlertId,
    pub rule: RuleId,
    #[serde(flatten)]
    pub state: AlertState,
    pub acknowledged: Option<Acknowledgement>,
}

/// What the alert engine keeps of an alert: everything but its id and acknowledgement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct AlertState {
    /// The rule's name when it fired.
    pub name: String,
    pub level: EventLevel,
    /// What it is about, in [`Entity`](crate::metric::Entity)'s text form: `node:3`,
    /// `node:3/radio:radio1`, `client:<mac>`, `link:<id>`, `site`.
    pub subject: String,
    pub node: Option<NodeId>,
    pub client: Option<MacAddr>,
    pub started_at: Timestamp,
    /// The latest time its rule matched.
    pub last_at: Timestamp,
    pub resolved_at: Option<Timestamp>,
    pub resolution: Option<AlertResolution>,
    /// How many times it fired: reopening within the rule's cool-down counts.
    pub count: u32,
    /// Fired during maintenance (a job on its node, or a window): recorded, nobody notified.
    pub silenced: bool,
    pub cause: AlertCause,
}

impl AlertState {
    pub const fn active(&self) -> bool {
        self.resolved_at.is_none()
    }
}

/// How the alert engine changes the alerts: by rule and subject, so the latest alert of the
/// pair is the one an update or a reopen changes.
#[derive(Debug, Clone, PartialEq)]
pub enum AlertWrite {
    /// A new alert.
    Open { rule: RuleId, state: AlertState },
    /// The latest alert of the pair, as it is now.
    Update { rule: RuleId, state: AlertState },
    /// The latest alert of the pair fired again within its cool-down: as `Update`, and its
    /// acknowledgement goes.
    Reopen { rule: RuleId, state: AlertState },
}

/// Which alerts to read: each part narrows.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(default)]
pub struct AlertFilter {
    /// Only alerts still firing.
    pub active: bool,
    pub min: Option<EventLevel>,
    pub nodes: Vec<NodeId>,
    pub rules: Vec<RuleId>,
    /// Started at or after `from`, and before `to`.
    pub from: Option<Timestamp>,
    pub to: Option<Timestamp>,
}

impl AlertFilter {
    pub fn admits(&self, alert: &AlertRecord) -> bool {
        let state = &alert.state;
        (!self.active || state.active())
            && self.min.is_none_or(|min| state.level <= min)
            && (self.nodes.is_empty() || state.node.is_some_and(|n| self.nodes.contains(&n)))
            && (self.rules.is_empty() || self.rules.contains(&alert.rule))
            && self.from.is_none_or(|from| state.started_at >= from)
            && self.to.is_none_or(|to| state.started_at < to)
    }
}

/// A maintenance window: alerts on its nodes (every node's, and those of no node, when it names
/// none) are silenced between its times.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct MaintenanceWindow {
    pub id: WindowId,
    pub starts_at: Timestamp,
    pub ends_at: Timestamp,
    pub nodes: Vec<NodeId>,
    pub note: String,
    pub created_by: Option<UserId>,
}

impl MaintenanceWindow {
    /// Whether it silences an alert about `node` at `at`.
    pub fn covers(&self, node: Option<NodeId>, at: Timestamp) -> bool {
        self.starts_at <= at
            && at < self.ends_at
            && (self.nodes.is_empty() || node.is_some_and(|node| self.nodes.contains(&node)))
    }
}

/// A maintenance window as users create it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct NewWindow {
    pub starts_at: Timestamp,
    pub ends_at: Timestamp,
    #[serde(default)]
    pub nodes: Vec<NodeId>,
    #[serde(default)]
    pub note: String,
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use uniwrt_core::{AlertId, EventId, NodeId, RuleId, Timestamp, WindowId};

    use super::{AlertCause, AlertFilter, AlertRecord, AlertState, MaintenanceWindow};
    use crate::events::EventLevel;

    fn alert() -> AlertRecord {
        AlertRecord {
            id: AlertId::new(7),
            rule: RuleId::new(2),
            state: AlertState {
                name: "Node offline".to_owned(),
                level: EventLevel::Critical,
                subject: "node:3".to_owned(),
                node: Some(NodeId::new(3)),
                client: None,
                started_at: Timestamp::from_unix_ms(1_000),
                last_at: Timestamp::from_unix_ms(1_000),
                resolved_at: None,
                resolution: None,
                count: 1,
                silenced: false,
                cause: AlertCause::Event {
                    event: EventId::new(9),
                    kind: "node_offline".to_owned(),
                    matches: 1,
                    condition: Some("node:3/offline".to_owned()),
                },
            },
            acknowledged: None,
        }
    }

    #[test]
    fn serializes_flat() {
        let value = serde_json::to_value(alert()).unwrap();
        assert_eq!(value["id"], 7);
        assert_eq!(value["subject"], "node:3");
        assert_eq!(
            value["cause"],
            json!({ "type": "event", "event": 9, "kind": "node_offline", "matches": 1,
                    "condition": "node:3/offline" })
        );
        let back: AlertRecord = serde_json::from_value(value).unwrap();
        assert_eq!(back, alert());
    }

    #[test]
    fn filters_and_windows() {
        let alert = alert();
        assert!(
            AlertFilter {
                active: true,
                ..AlertFilter::default()
            }
            .admits(&alert)
        );
        assert!(
            !AlertFilter {
                nodes: vec![NodeId::new(4)],
                ..AlertFilter::default()
            }
            .admits(&alert)
        );
        assert!(
            !AlertFilter {
                rules: vec![RuleId::new(1)],
                ..AlertFilter::default()
            }
            .admits(&alert)
        );
        let window = MaintenanceWindow {
            id: WindowId::new(1),
            starts_at: Timestamp::from_unix_ms(0),
            ends_at: Timestamp::from_unix_ms(2_000),
            nodes: vec![NodeId::new(3)],
            note: String::new(),
            created_by: None,
        };
        assert!(window.covers(Some(NodeId::new(3)), Timestamp::from_unix_ms(1_999)));
        assert!(!window.covers(Some(NodeId::new(3)), Timestamp::from_unix_ms(2_000)));
        assert!(!window.covers(None, Timestamp::from_unix_ms(10)));
        let all = MaintenanceWindow {
            nodes: Vec::new(),
            ..window
        };
        assert!(all.covers(None, Timestamp::from_unix_ms(10)));
    }
}
