//! The ops that make the agent push ([`AgentEvent`](crate::AgentEvent)s): `subscribe` and
//! `unsubscribe` (ubus notifications), `log_follow` and `set_interval` (ticks), mirrored by
//! `node-agent/streams.uc`, `logs.uc` and `ticks.uc`. They need the agent's event loop
//! ([`AgentCaps::events`](crate::proto::AgentCaps::events)); without it they fail with
//! [`code::UNSUPPORTED`](super::code::UNSUPPORTED) and the engine polls instead. What the agent
//! pushes is parsed by [`events`](crate::events). Pushes stop with the agent, so after a restart
//! (a new hello) the engine asks again.

use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Serialize};

use super::{Op, Set};

/// A source of push events, named as in [`AgentCaps::topics`](crate::proto::AgentCaps::topics).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Topic {
    /// Wi-Fi clients of each access point: `hostapd.auth`, `.assoc`, `.sta-authorized`,
    /// `.disassoc` and the rest of hostapd's notifications but probe requests. The agent
    /// subscribes to each BSS's object as it appears.
    Hostapd,
    /// `netifd.interface.update` and `.interface.down` (with the interface's status), and
    /// `netifd.device.<change>` (devices and their links).
    Netifd,
    /// `dnsmasq.dhcp.ack` and `.dhcp.release`, and odhcpd's as `odhcpd.<type>`.
    Dhcp,
}

impl Topic {
    pub const ALL: [Self; 3] = [Self::Dhcp, Self::Hostapd, Self::Netifd];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dhcp => "dhcp",
            Self::Hostapd => "hostapd",
            Self::Netifd => "netifd",
        }
    }
}

impl fmt::Display for Topic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// `subscribe`: adds topics; subscribing to an active one changes nothing.
#[derive(Debug, Clone, Serialize)]
pub struct Subscribe {
    pub topics: Vec<Topic>,
}

impl Op for Subscribe {
    const NAME: &'static str = "subscribe";
    type Output = Subscribed;
}

/// `unsubscribe`: removes topics.
#[derive(Debug, Clone, Serialize)]
pub struct Unsubscribe {
    pub topics: Vec<Topic>,
}

impl Op for Unsubscribe {
    const NAME: &'static str = "unsubscribe";
    type Output = Subscribed;
}

/// The topics active after a `subscribe` or `unsubscribe`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Subscribed {
    pub topics: Vec<Topic>,
}

/// `log_follow`: pushes the node's log as `log` events, replacing any earlier follow
/// ([`LogMode`]); `backfill` entries logged before go first.
#[derive(Debug, Clone, Serialize)]
pub struct LogFollow {
    pub mode: LogMode,
    /// At most [`LogFollow::MAX_BACKFILL`].
    pub backfill: u32,
    /// How often [`LogMode::Poll`] reads logd.
    pub interval_ms: u64,
}

impl LogFollow {
    pub const MAX_BACKFILL: u32 = 1000;
    pub const DEFAULT_INTERVAL_MS: u64 = 1000;

    pub const fn new(mode: LogMode) -> Self {
        Self {
            mode,
            backfill: 0,
            interval_ms: Self::DEFAULT_INTERVAL_MS,
        }
    }
}

impl Op for LogFollow {
    const NAME: &'static str = "log_follow";
    type Output = LogFollowing;
}

/// How the agent follows the log (D-036).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogMode {
    /// Reads logd over ubus every `interval_ms`: entries keep logd's sequence numbers and
    /// millisecond times, and a gap is counted as dropped. The default.
    Poll,
    /// Runs `logread -f` and pushes its lines as they come (within 100 ms), at the cost of two
    /// processes on the node; lines have no sequence numbers.
    Stream,
    /// Stops following.
    Off,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct LogFollowing {
    pub mode: LogMode,
}

/// `set_interval`: collects `sets` every `interval_ms` and pushes them as ticks, each tick
/// carrying every set of its interval; 0 stops collecting them. A set has one interval at a
/// time, so this moves it from any other. The interval is at least
/// [`SetInterval::MIN_MS`].
#[derive(Debug, Clone, Serialize)]
pub struct SetInterval {
    pub interval_ms: u64,
    pub sets: Vec<Set>,
}

impl SetInterval {
    pub const MIN_MS: u64 = 1000;
    pub const MAX_MS: u64 = 86_400_000;
}

impl Op for SetInterval {
    const NAME: &'static str = "set_interval";
    type Output = Schedule;
}

/// Every scheduled set's interval, after a `set_interval`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Schedule {
    pub intervals: BTreeMap<Set, u64>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{LogFollow, LogMode, Schedule, Set, SetInterval, Subscribe, Topic};

    #[test]
    fn requests_serialize_as_the_agent_reads_them() {
        assert_eq!(
            serde_json::to_value(Subscribe {
                topics: Topic::ALL.to_vec()
            })
            .unwrap(),
            json!({"topics": ["dhcp", "hostapd", "netifd"]})
        );
        assert_eq!(
            serde_json::to_value(LogFollow::new(LogMode::Poll)).unwrap(),
            json!({"mode": "poll", "backfill": 0, "interval_ms": 1000})
        );
        assert_eq!(
            serde_json::to_value(SetInterval {
                interval_ms: 2000,
                sets: vec![Set::Stations]
            })
            .unwrap(),
            json!({"interval_ms": 2000, "sets": ["stations"]})
        );
    }

    #[test]
    fn topic_names_match_serde() {
        for topic in Topic::ALL {
            assert_eq!(serde_json::to_value(topic).unwrap(), json!(topic.as_str()));
        }
    }

    #[test]
    fn an_empty_schedule_is_an_empty_object() {
        // ucode prints an empty object for no intervals.
        let schedule: Schedule = serde_json::from_str(r#"{"intervals":{}}"#).unwrap();
        assert!(schedule.intervals.is_empty());
        let schedule: Schedule = serde_json::from_str(r#"{"intervals":{"system":15000}}"#).unwrap();
        assert_eq!(schedule.intervals[&Set::System], 15_000);
    }
}
