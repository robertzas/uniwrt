//! Parsing what the agent pushes ([`AgentEvent`]): ticks into [`Collected`] (applied with
//! [`facts::apply`](crate::facts::apply)), the notifications of the subscribed
//! [`Topic`](crate::proto::ops::Topic)s into [`NodeEvent`]s, and `log` batches into
//! [`LogEntry`]s. An event is named `<source>.<type>` after the daemon's ubus notification, and
//! its data is the notification's (`node-agent/streams.uc`). Like the facts parsers, these are
//! pure, lenient where OpenWrt versions differ and strict where the shape is wrong.

mod dhcp;
mod hostapd;
mod log;
mod netifd;
#[cfg(test)]
mod tests;

use serde::{Deserialize, de::DeserializeOwned};
use uniwrt_model::{event::NodeEvent, log::LogEntry};

use crate::proto::{AgentEvent, ops::Collected};

/// One push from the agent, parsed.
#[derive(Debug)]
pub enum Push {
    /// Periodic collection ([`SetInterval`](crate::proto::ops::SetInterval)).
    Tick(Collected),
    Event(NodeEvent),
    /// Log entries, oldest first, and how many were lost before them: skipped by a poll that
    /// fell behind, or lines that did not parse.
    Log {
        entries: Vec<LogEntry>,
        dropped: u64,
    },
    /// Following the log stopped on the node: its `logread` ended.
    LogEnded {
        reason: String,
    },
}

/// A push that has the wrong shape.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("agent event {topic}: {reason}")]
pub struct EventError {
    pub topic: String,
    pub reason: String,
}

#[derive(Deserialize)]
struct LogEnd {
    reason: String,
}

/// Parses one push: `None` for what this controller does not model, such as hostapd's beacon
/// reports or a newer agent's topics.
pub fn parse(event: &AgentEvent) -> Result<Option<Push>, EventError> {
    let (topic, raw) = match event {
        AgentEvent::Tick(raw) => return error("tick", decode(raw.get())).map(Push::Tick).map(Some),
        AgentEvent::Event { topic, data, .. } => (topic.as_str(), data.get()),
    };
    let parsed = match topic {
        "log" => decode(raw).map(|raw| {
            let (entries, dropped) = log::parse(raw);
            Some(Push::Log { entries, dropped })
        }),
        "log.end" => decode(raw).map(|end: LogEnd| Some(Push::LogEnded { reason: end.reason })),
        _ => {
            let (source, kind) = split(topic);
            match source {
                "hostapd" => hostapd::parse(kind, raw),
                "netifd" => netifd::interface(kind, raw),
                "netifd.device" => netifd::device(kind, raw),
                "dnsmasq" | "odhcpd" => dhcp::parse(kind, raw),
                _ => Ok(None),
            }
            .map(|event| event.map(Push::Event))
        }
    };
    error(topic, parsed)
}

/// `hostapd.sta-authorized` → (`hostapd`, `sta-authorized`); a source has at most two parts
/// and a type may have dots: `netifd.device.link_up`, `netifd.interface.update`,
/// `dnsmasq.dhcp.ack`.
fn split(topic: &str) -> (&str, &str) {
    for source in ["netifd.device", "hostapd", "netifd", "dnsmasq", "odhcpd"] {
        if let Some(kind) = topic
            .strip_prefix(source)
            .and_then(|rest| rest.strip_prefix('.'))
        {
            return (source, kind);
        }
    }
    (topic, "")
}

fn decode<T: DeserializeOwned>(raw: &str) -> Result<T, String> {
    serde_json::from_str(raw).map_err(|err| err.to_string())
}

fn error<T>(topic: &str, result: Result<T, String>) -> Result<T, EventError> {
    result.map_err(|reason| EventError {
        topic: topic.to_owned(),
        reason,
    })
}
