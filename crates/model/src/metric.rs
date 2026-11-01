//! Metrics (features/observability.md, Metrics): series of numbers over time, each named by
//! what it measures ([`Entity`]) and what it measures of it ([`Metric`]), stored as
//! [`Sample`]s and read back as [`Point`]s at a [`Resolution`].

use std::{fmt, str::FromStr, time::Duration};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uniwrt_core::{MacAddr, NodeId, Timestamp};

/// What a series measures. Its text form (`node:3`, `node:3/dev:br-lan`, `node:3/radio:radio1`,
/// `link:<topology edge id>`, `client:<mac>`, `site`) is how the store and the API name it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Entity {
    /// The whole network: WAN traffic, clients and nodes online.
    Site,
    Node(NodeId),
    /// A kernel network device of a node.
    Device {
        node: NodeId,
        name: String,
    },
    /// A radio of a node, by its UCI name.
    Radio {
        node: NodeId,
        name: String,
    },
    /// A link of the topology (a mesh link), by its edge id.
    Link(String),
    Client(MacAddr),
}

impl Entity {
    /// The node it belongs to.
    pub fn node(&self) -> Option<NodeId> {
        match self {
            Self::Node(node) | Self::Device { node, .. } | Self::Radio { node, .. } => Some(*node),
            Self::Site | Self::Link(_) | Self::Client(_) => None,
        }
    }
}

impl fmt::Display for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Site => f.write_str("site"),
            Self::Node(node) => write!(f, "node:{node}"),
            Self::Device { node, name } => write!(f, "node:{node}/dev:{name}"),
            Self::Radio { node, name } => write!(f, "node:{node}/radio:{name}"),
            Self::Link(id) => write!(f, "link:{id}"),
            Self::Client(mac) => write!(f, "client:{mac}"),
        }
    }
}

/// Text that names no entity.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("not a metrics entity: {0:?}")]
pub struct ParseEntityError(String);

impl FromStr for Entity {
    type Err = ParseEntityError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let fail = || ParseEntityError(text.to_owned());
        // A name: non-empty, printable, and without the separator.
        let name = |name: &str| {
            (!name.is_empty()
                && name.len() <= 64
                && !name.contains('/')
                && !name.contains(char::is_control))
            .then(|| name.to_owned())
            .ok_or_else(fail)
        };
        if text == "site" {
            return Ok(Self::Site);
        }
        if let Some(id) = text.strip_prefix("link:") {
            return (!id.is_empty() && id.len() <= 256 && !id.contains(char::is_control))
                .then(|| Self::Link(id.to_owned()))
                .ok_or_else(fail);
        }
        if let Some(mac) = text.strip_prefix("client:") {
            return mac.parse().map(Self::Client).map_err(|_| fail());
        }
        let rest = text.strip_prefix("node:").ok_or_else(fail)?;
        let (id, part) = rest
            .split_once('/')
            .map_or((rest, None), |(id, part)| (id, Some(part)));
        let node = id
            .parse::<i64>()
            .ok()
            .filter(|&raw| raw > 0 && !id.starts_with(['+', '0']))
            .map(NodeId::new)
            .ok_or_else(fail)?;
        match part {
            None => Ok(Self::Node(node)),
            Some(part) => {
                if let Some(dev) = part.strip_prefix("dev:") {
                    Ok(Self::Device {
                        node,
                        name: name(dev)?,
                    })
                } else if let Some(radio) = part.strip_prefix("radio:") {
                    Ok(Self::Radio {
                        node,
                        name: name(radio)?,
                    })
                } else {
                    Err(fail())
                }
            }
        }
    }
}

impl Serialize for Entity {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Entity {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = <std::borrow::Cow<'de, str>>::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(feature = "ts")]
impl ts_rs::TS for Entity {
    type WithoutGenerics = Self;
    type OptionInnerType = Self;

    fn name(_: &ts_rs::Config) -> String {
        "string".to_owned()
    }

    fn inline(_: &ts_rs::Config) -> String {
        "string".to_owned()
    }
}

/// What a series measures of its entity; the unit is in [`Metric::unit`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    /// A node's CPU use.
    Cpu,
    /// A node's memory in use (not available).
    Memory,
    /// A node's 1-minute load average.
    Load,
    /// A node's temperature.
    Temperature,
    /// Bits per second a device received.
    RxBps,
    /// Bits per second a device sent.
    TxBps,
    /// A radio's channel busy share.
    Busy,
    /// A radio's noise floor.
    Noise,
    /// Wi-Fi clients of a node or radio.
    Stations,
    /// A client's signal at its access point, or a mesh link's weaker end's.
    Signal,
    /// A client's signal over its radio's noise.
    Snr,
    /// The rate an access point sends to a client at.
    TxRate,
    /// The rate a client sends to its access point at.
    RxRate,
    /// Bits per second toward a client, or into the network over the WAN.
    DownBps,
    /// Bits per second from a client, or out of the network over the WAN.
    UpBps,
    /// A client's experience score.
    Score,
    /// A link's quality.
    Quality,
    /// What a link can carry.
    Capacity,
    /// batman-adv's transmit quality of a link.
    Tq,
    /// Clients online.
    Clients,
    /// Adopted nodes online.
    NodesOnline,
}

/// The unit of a metric's values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    Percent,
    Load,
    Celsius,
    BitsPerSecond,
    Kbps,
    Mbps,
    Dbm,
    Db,
    Count,
    /// 0–100, higher is better.
    Score,
    /// batman-adv's 0–255.
    Tq,
}

impl Metric {
    pub const ALL: [Self; 21] = [
        Self::Cpu,
        Self::Memory,
        Self::Load,
        Self::Temperature,
        Self::RxBps,
        Self::TxBps,
        Self::Busy,
        Self::Noise,
        Self::Stations,
        Self::Signal,
        Self::Snr,
        Self::TxRate,
        Self::RxRate,
        Self::DownBps,
        Self::UpBps,
        Self::Score,
        Self::Quality,
        Self::Capacity,
        Self::Tq,
        Self::Clients,
        Self::NodesOnline,
    ];

    /// Its serde name (`rx_bps`), as the store keeps it.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Memory => "memory",
            Self::Load => "load",
            Self::Temperature => "temperature",
            Self::RxBps => "rx_bps",
            Self::TxBps => "tx_bps",
            Self::Busy => "busy",
            Self::Noise => "noise",
            Self::Stations => "stations",
            Self::Signal => "signal",
            Self::Snr => "snr",
            Self::TxRate => "tx_rate",
            Self::RxRate => "rx_rate",
            Self::DownBps => "down_bps",
            Self::UpBps => "up_bps",
            Self::Score => "score",
            Self::Quality => "quality",
            Self::Capacity => "capacity",
            Self::Tq => "tq",
            Self::Clients => "clients",
            Self::NodesOnline => "nodes_online",
        }
    }

    pub const fn unit(self) -> Unit {
        match self {
            Self::Cpu | Self::Memory | Self::Busy => Unit::Percent,
            Self::Load => Unit::Load,
            Self::Temperature => Unit::Celsius,
            Self::RxBps | Self::TxBps | Self::DownBps | Self::UpBps => Unit::BitsPerSecond,
            Self::TxRate | Self::RxRate => Unit::Kbps,
            Self::Capacity => Unit::Mbps,
            Self::Noise | Self::Signal => Unit::Dbm,
            Self::Snr => Unit::Db,
            Self::Stations | Self::Clients | Self::NodesOnline => Unit::Count,
            Self::Score | Self::Quality => Unit::Score,
            Self::Tq => Unit::Tq,
        }
    }
}

impl fmt::Display for Metric {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Text that names no metric.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("not a metric: {0:?}")]
pub struct ParseMetricError(String);

impl FromStr for Metric {
    type Err = ParseMetricError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|metric| metric.name() == text)
            .ok_or_else(|| ParseMetricError(text.to_owned()))
    }
}

/// One value of a series: the average over the window that starts at `at`.
#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    pub entity: Entity,
    pub metric: Metric,
    pub at: Timestamp,
    pub value: f64,
}

/// How finely a series is read: the raw windows, or a rollup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Resolution {
    Raw,
    Minute,
    Hour,
}

impl Resolution {
    /// How far apart its points are: raw windows are 15 s.
    pub const fn step(self) -> Duration {
        match self {
            Self::Raw => Duration::from_secs(15),
            Self::Minute => Duration::from_mins(1),
            Self::Hour => Duration::from_hours(1),
        }
    }

    /// The next finer one.
    pub const fn finer(self) -> Option<Self> {
        match self {
            Self::Raw => None,
            Self::Minute => Some(Self::Raw),
            Self::Hour => Some(Self::Minute),
        }
    }
}

/// A series' value over the time from `at` to the next point: for raw windows `min`, `avg`
/// and `max` are the one value; a rollup also keeps the last.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub at: Timestamp,
    pub min: f64,
    pub avg: f64,
    pub max: f64,
}

impl Point {
    pub const fn raw(at: Timestamp, value: f64) -> Self {
        Self {
            at,
            min: value,
            avg: value,
            max: value,
        }
    }
}

#[cfg(test)]
mod tests {
    use uniwrt_core::NodeId;

    use super::{Entity, Metric};

    #[test]
    fn entities_round_trip_through_text() {
        let node = NodeId::new(3);
        for (entity, text) in [
            (Entity::Site, "site"),
            (Entity::Node(node), "node:3"),
            (
                Entity::Device {
                    node,
                    name: "br-lan".to_owned(),
                },
                "node:3/dev:br-lan",
            ),
            (
                Entity::Radio {
                    node,
                    name: "radio1".to_owned(),
                },
                "node:3/radio:radio1",
            ),
            (
                Entity::Link("node:1~node:2~mesh~5g".to_owned()),
                "link:node:1~node:2~mesh~5g",
            ),
            (
                Entity::Client("f0:18:98:00:00:01".parse().unwrap()),
                "client:f0:18:98:00:00:01",
            ),
        ] {
            assert_eq!(entity.to_string(), text);
            assert_eq!(text.parse::<Entity>().unwrap(), entity);
            assert_eq!(
                serde_json::to_string(&entity).unwrap(),
                format!("\"{text}\"")
            );
        }
    }

    #[test]
    fn malformed_entities_are_refused() {
        for text in [
            "",
            "sites",
            "node:",
            "node:0",
            "node:03",
            "node:+3",
            "node:x",
            "node:3/",
            "node:3/dev:",
            "node:3/dev:a/b",
            "node:3/port:lan1",
            "client:nope",
            "link:",
        ] {
            assert!(text.parse::<Entity>().is_err(), "{text:?}");
        }
    }

    #[test]
    fn metric_names_are_their_serde_names() {
        for metric in Metric::ALL {
            assert_eq!(
                serde_json::to_string(&metric).unwrap(),
                format!("\"{}\"", metric.name())
            );
            assert_eq!(metric.name().parse::<Metric>().unwrap(), metric);
        }
    }
}
