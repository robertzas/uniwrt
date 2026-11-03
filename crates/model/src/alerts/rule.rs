//! Alert rules: when an alert fires. A metric rule watches stored series (a threshold crossed
//! for a while); an event rule watches recorded events (a kind, node or level, some number of
//! times within a window). Built-in rules are presets of the same two kinds.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uniwrt_core::{MacAddr, NodeId, RuleId};

use crate::{
    events::{EventCategory, EventLevel},
    metric::{Entity, Metric},
};

/// An alert rule as stored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct AlertRule {
    pub id: RuleId,
    /// The built-in rule it is (`alert-rules.toml`), which it can be reset to; `None` for the
    /// user's own.
    pub preset: Option<String>,
    #[serde(flatten)]
    pub spec: RuleSpec,
}

/// What a rule says, as users write it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RuleSpec {
    pub name: String,
    pub enabled: bool,
    /// How much its alerts matter.
    pub level: EventLevel,
    pub when: RuleCondition,
    /// After an alert resolves, the rule firing again for the same subject within this long
    /// reopens it instead of raising a new one.
    pub cooldown_s: u32,
}

/// The longest a rule's durations may be: a day for `for_s`, a week for windows and cool-downs.
pub const MAX_FOR_S: u32 = 24 * 3600;
pub const MAX_WINDOW_S: u32 = 7 * 24 * 3600;
const MAX_NAME: usize = 80;
const MAX_VALUES: usize = 32;

impl RuleSpec {
    /// Checks it, saying what is wrong in words.
    pub fn validate(&self) -> Result<(), String> {
        let name = self.name.trim();
        if name.is_empty() || name.chars().count() > MAX_NAME || name.contains(char::is_control) {
            return Err(format!("A rule's name has 1 to {MAX_NAME} characters"));
        }
        if self.cooldown_s > MAX_WINDOW_S {
            return Err("The cool-down is at most a week".to_owned());
        }
        match &self.when {
            RuleCondition::Metric(metric) => metric.validate(),
            RuleCondition::Event(event) => event.validate(),
        }
    }
}

/// What makes a rule fire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuleCondition {
    Metric(MetricCondition),
    Event(EventCondition),
}

/// A series beyond a threshold for a while: each matching series is a subject of its own.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct MetricCondition {
    pub metric: Metric,
    /// Which series, as an [`EntityPattern`]: `node:*`, `node:3`, `client:*`, `link:*`, `site`,
    /// a radio or device of any node, …
    pub entities: EntityPattern,
    pub op: Comparison,
    pub threshold: f64,
    /// Back to normal once on the other side of this (hysteresis); the threshold when `None`.
    pub clear: Option<f64>,
    /// How long it must stay beyond the threshold; 0 fires on the first window.
    pub for_s: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Comparison {
    Above,
    Below,
}

impl Comparison {
    /// Whether `value` is beyond `threshold`.
    pub fn beyond(self, value: f64, threshold: f64) -> bool {
        match self {
            Self::Above => value > threshold,
            Self::Below => value < threshold,
        }
    }
}

impl MetricCondition {
    /// The value it is back to normal at.
    pub fn clear_at(&self) -> f64 {
        self.clear.unwrap_or(self.threshold)
    }

    fn validate(&self) -> Result<(), String> {
        if !self.threshold.is_finite() || !self.clear_at().is_finite() {
            return Err("The threshold is a number".to_owned());
        }
        if self.op.beyond(self.clear_at(), self.threshold) {
            return Err(match self.op {
                Comparison::Above => "The value it clears at is at most the threshold",
                Comparison::Below => "The value it clears at is at least the threshold",
            }
            .to_owned());
        }
        if self.for_s > MAX_FOR_S {
            return Err("A metric rule waits at most a day".to_owned());
        }
        if !self.entities.measures(self.metric) {
            return Err(format!("{} has no {} series", self.entities, self.metric));
        }
        Ok(())
    }
}

/// Events that match, some number of times within a window, for one subject (their node, else
/// their client, else the site).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct EventCondition {
    /// Event kinds by name (`node_offline`); empty matches any.
    #[serde(default)]
    pub kinds: Vec<String>,
    #[serde(default)]
    pub categories: Vec<EventCategory>,
    /// The least level that matches.
    #[serde(default)]
    pub min: Option<EventLevel>,
    /// Only these nodes' events; empty matches any.
    #[serde(default)]
    pub nodes: Vec<NodeId>,
    /// For log patterns' events, only these patterns (`log-patterns.toml` ids); empty matches
    /// any.
    #[serde(default)]
    pub patterns: Vec<String>,
    /// How many matches fire it (at least 1)…
    pub count: u32,
    /// …within this long. With a count of 1, an event that raises a condition (a node offline)
    /// keeps its alert until the condition clears; otherwise the alert resolves once the window
    /// holds fewer matches.
    pub window_s: u32,
}

impl EventCondition {
    fn validate(&self) -> Result<(), String> {
        if self.count == 0 || self.count > 10_000 {
            return Err("An event rule counts from 1 to 10,000 events".to_owned());
        }
        if self.window_s == 0 || self.window_s > MAX_WINDOW_S {
            return Err("An event rule's window is from a second to a week".to_owned());
        }
        let lists = [self.kinds.len(), self.nodes.len(), self.patterns.len()];
        if lists.into_iter().any(|len| len > MAX_VALUES) {
            return Err(format!("A rule lists at most {MAX_VALUES} of each"));
        }
        let bad = |name: &String| name.is_empty() || name.len() > 64;
        if self.kinds.iter().chain(&self.patterns).any(bad) {
            return Err("Kinds and patterns are named".to_owned());
        }
        Ok(())
    }
}

/// Series a metric rule watches, by entity: each part is a value, or `*` for any. Its text form
/// follows [`Entity`]'s: `site`, `node:<id|*>`, `node:<id|*>/dev:<name|*>`,
/// `node:<id|*>/radio:<name|*>`, `link:<id|*>`, `client:<mac|*>`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EntityPattern {
    Site,
    Node(Option<NodeId>),
    Device {
        node: Option<NodeId>,
        name: Option<String>,
    },
    Radio {
        node: Option<NodeId>,
        name: Option<String>,
    },
    Link(Option<String>),
    Client(Option<MacAddr>),
}

impl EntityPattern {
    pub fn matches(&self, entity: &Entity) -> bool {
        let any = |want: Option<&str>, have: &str| want.is_none_or(|want| want == have);
        let node = |want: Option<NodeId>, have: NodeId| want.is_none_or(|want| want == have);
        match (self, entity) {
            (Self::Site, Entity::Site) => true,
            (Self::Node(want), Entity::Node(have)) => node(*want, *have),
            (
                Self::Device { node: n, name },
                Entity::Device {
                    node: hn,
                    name: hname,
                },
            )
            | (
                Self::Radio { node: n, name },
                Entity::Radio {
                    node: hn,
                    name: hname,
                },
            ) => node(*n, *hn) && any(name.as_deref(), hname),
            (Self::Link(want), Entity::Link(have)) => any(want.as_deref(), have),
            (Self::Client(want), Entity::Client(have)) => want.is_none_or(|want| want == *have),
            _ => false,
        }
    }

    /// Whether the entities it names have `metric`'s series.
    pub fn measures(&self, metric: Metric) -> bool {
        use Metric as M;
        match self {
            Self::Site => matches!(metric, M::DownBps | M::UpBps | M::Clients | M::NodesOnline),
            Self::Node(_) => matches!(
                metric,
                M::Cpu | M::Memory | M::Load | M::Temperature | M::Stations
            ),
            Self::Device { .. } => matches!(metric, M::RxBps | M::TxBps),
            Self::Radio { .. } => matches!(metric, M::Busy | M::Noise | M::Stations),
            Self::Link(_) => matches!(metric, M::Signal | M::Tq | M::Quality | M::Capacity),
            Self::Client(_) => matches!(
                metric,
                M::Signal | M::Snr | M::TxRate | M::RxRate | M::DownBps | M::UpBps | M::Score
            ),
        }
    }
}

impl fmt::Display for EntityPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn part<T: fmt::Display>(value: Option<&T>) -> String {
            value.map_or_else(|| "*".to_owned(), ToString::to_string)
        }
        match self {
            Self::Site => f.write_str("site"),
            Self::Node(node) => write!(f, "node:{}", part(node.as_ref())),
            Self::Device { node, name } => {
                write!(
                    f,
                    "node:{}/dev:{}",
                    part(node.as_ref()),
                    part(name.as_ref())
                )
            }
            Self::Radio { node, name } => {
                write!(
                    f,
                    "node:{}/radio:{}",
                    part(node.as_ref()),
                    part(name.as_ref())
                )
            }
            Self::Link(id) => write!(f, "link:{}", part(id.as_ref())),
            Self::Client(mac) => write!(f, "client:{}", part(mac.as_ref())),
        }
    }
}

/// Text that names no entity pattern.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("not an entity pattern: {0:?}")]
pub struct ParsePatternError(String);

impl FromStr for EntityPattern {
    type Err = ParsePatternError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        fn star(part: &str) -> Option<&str> {
            (part != "*").then_some(part)
        }
        let fail = || ParsePatternError(text.to_owned());
        // `*` stands in for any value; a concrete one parses as an entity would.
        let entity = |text: &str| text.parse::<Entity>().map_err(|_| fail());
        if text == "site" {
            return Ok(Self::Site);
        }
        if let Some(id) = text.strip_prefix("link:") {
            return match star(id) {
                None => Ok(Self::Link(None)),
                Some(_) => match entity(text)? {
                    Entity::Link(id) => Ok(Self::Link(Some(id))),
                    _ => Err(fail()),
                },
            };
        }
        if let Some(mac) = text.strip_prefix("client:") {
            return match star(mac) {
                None => Ok(Self::Client(None)),
                Some(mac) => mac
                    .parse()
                    .map(|mac| Self::Client(Some(mac)))
                    .map_err(|_| fail()),
            };
        }
        let rest = text.strip_prefix("node:").ok_or_else(fail)?;
        let (id, part) = rest
            .split_once('/')
            .map_or((rest, None), |(id, part)| (id, Some(part)));
        // A concrete node id parses as `node:<id>` does.
        let node = match star(id) {
            None => None,
            Some(id) => match entity(&format!("node:{id}"))? {
                Entity::Node(node) => Some(node),
                _ => return Err(fail()),
            },
        };
        let name = |kind: &str, name: &str| -> Result<Option<String>, ParsePatternError> {
            match star(name) {
                None => Ok(None),
                Some(name) => {
                    entity(&format!("node:1/{kind}:{name}"))?;
                    Ok(Some(name.to_owned()))
                }
            }
        };
        match part {
            None => Ok(Self::Node(node)),
            Some(part) => {
                if let Some(dev) = part.strip_prefix("dev:") {
                    Ok(Self::Device {
                        node,
                        name: name("dev", dev)?,
                    })
                } else if let Some(radio) = part.strip_prefix("radio:") {
                    Ok(Self::Radio {
                        node,
                        name: name("radio", radio)?,
                    })
                } else {
                    Err(fail())
                }
            }
        }
    }
}

impl Serialize for EntityPattern {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for EntityPattern {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = <std::borrow::Cow<'de, str>>::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(feature = "ts")]
impl ts_rs::TS for EntityPattern {
    type WithoutGenerics = Self;
    type OptionInnerType = Self;

    fn name(_: &ts_rs::Config) -> String {
        "string".to_owned()
    }

    fn inline(_: &ts_rs::Config) -> String {
        "string".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use uniwrt_core::NodeId;

    use super::{
        Comparison, EntityPattern, EventCondition, MetricCondition, RuleCondition, RuleSpec,
    };
    use crate::{
        events::EventLevel,
        metric::{Entity, Metric},
    };

    #[test]
    fn patterns_round_trip_and_match() {
        for text in [
            "site",
            "node:*",
            "node:3",
            "node:*/dev:*",
            "node:3/dev:eth1",
            "node:*/radio:radio1",
            "link:*",
            "link:node:1~node:2~wifi~5g",
            "client:*",
            "client:f0:18:98:00:00:01",
        ] {
            let pattern: EntityPattern = text.parse().unwrap();
            assert_eq!(pattern.to_string(), text);
        }
        for bad in [
            "",
            "node:",
            "node:0",
            "node:x",
            "node:*/dev:",
            "node:*/eth:1",
            "client:x",
            "sites",
        ] {
            assert!(bad.parse::<EntityPattern>().is_err(), "{bad:?}");
        }
        let radio: Entity = "node:3/radio:radio1".parse().unwrap();
        let matches = |pattern: &str| pattern.parse::<EntityPattern>().unwrap().matches(&radio);
        assert!(matches("node:*/radio:*"));
        assert!(matches("node:3/radio:radio1"));
        assert!(!matches("node:4/radio:*"));
        assert!(!matches("node:*/radio:radio0"));
        assert!(!matches("node:*"));
        assert!(!matches("node:*/dev:*"));
    }

    fn spec(when: RuleCondition) -> RuleSpec {
        RuleSpec {
            name: "Busy".to_owned(),
            enabled: true,
            level: EventLevel::Warning,
            when,
            cooldown_s: 600,
        }
    }

    #[test]
    fn validates_in_words() {
        let metric = |entities: &str, clear: Option<f64>| {
            spec(RuleCondition::Metric(MetricCondition {
                metric: Metric::Cpu,
                entities: entities.parse().unwrap(),
                op: Comparison::Above,
                threshold: 90.0,
                clear,
                for_s: 300,
            }))
        };
        assert_eq!(metric("node:*", Some(80.0)).validate(), Ok(()));
        assert!(metric("node:*", Some(95.0)).validate().is_err());
        assert!(metric("client:*", None).validate().is_err());
        let event = |count, window_s| {
            spec(RuleCondition::Event(EventCondition {
                kinds: vec!["node_offline".to_owned()],
                categories: Vec::new(),
                min: None,
                nodes: vec![NodeId::new(1)],
                patterns: Vec::new(),
                count,
                window_s,
            }))
        };
        assert_eq!(event(1, 60).validate(), Ok(()));
        assert!(event(0, 60).validate().is_err());
        assert!(event(1, 0).validate().is_err());
        let mut unnamed = event(1, 60);
        unnamed.name = "  ".to_owned();
        assert!(unnamed.validate().is_err());
    }

    #[test]
    fn serializes_tagged() {
        let rule = metric_json();
        let back: RuleSpec = serde_json::from_value(rule.clone()).unwrap();
        assert_eq!(serde_json::to_value(&back).unwrap(), rule);
    }

    fn metric_json() -> serde_json::Value {
        serde_json::json!({
            "name": "Hot", "enabled": true, "level": "warning", "cooldown_s": 600,
            "when": {
                "type": "metric", "metric": "temperature", "entities": "node:*", "op": "above",
                "threshold": 85.0, "clear": null, "for_s": 300,
            },
        })
    }
}
