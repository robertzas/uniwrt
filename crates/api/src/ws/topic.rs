//! WS topics as clients name them: `fleet`, `node:<id>`, `logs` or
//! `logs?node=<id>|host=<name>&min=<severity>` (each parameter optional, in any order: every
//! source and every entry by default), `events` or `events?node=<id>&min=<level>` (likewise), `alerts`,
//! `jobs`, `topology`, `clients` and `client:<mac>`.

use std::fmt;

use uniwrt_core::{MacAddr, NodeId};
use uniwrt_model::{events::EventLevel, log::Severity};

/// The longest topic name accepted.
pub(crate) const MAX_LEN: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Topic {
    /// Every node's summary.
    Fleet,
    /// One node: its status, facts, events and roams; viewing it speeds up its telemetry.
    Node(NodeId),
    /// Stored log entries at `min` or more severe: every source's, or one node's or host's.
    Logs {
        node: Option<NodeId>,
        host: Option<String>,
        min: Severity,
    },
    /// Recorded events at `min` or more: every node's, or one node's.
    Events {
        node: Option<NodeId>,
        min: EventLevel,
    },
    /// The alerts firing, then every change to an alert.
    Alerts,
    /// The latest jobs, and every change to a job.
    Jobs,
    /// The fleet's topology, then what changes in it.
    Topology,
    /// Every client, then the ones that change.
    Clients,
    /// One client: its view, samples and sessions; viewing it speeds up its node's telemetry.
    Client(MacAddr),
    /// RF insight's view, whole, then again whenever it changes.
    Rf,
}

/// Why a topic name is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BadTopic(pub(crate) String);

impl fmt::Display for BadTopic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Topic {
    pub(crate) fn parse(name: &str) -> Result<Self, BadTopic> {
        if name.len() > MAX_LEN {
            return Err(BadTopic(format!("A topic is at most {MAX_LEN} bytes long")));
        }
        if name == "fleet" {
            return Ok(Self::Fleet);
        }
        if name == "jobs" {
            return Ok(Self::Jobs);
        }
        if name == "alerts" {
            return Ok(Self::Alerts);
        }
        if name == "topology" {
            return Ok(Self::Topology);
        }
        if name == "clients" {
            return Ok(Self::Clients);
        }
        if name == "rf" {
            return Ok(Self::Rf);
        }
        if let Some(mac) = name.strip_prefix("client:") {
            return mac
                .parse()
                .map(Self::Client)
                .map_err(|_| BadTopic(format!("{mac:?} is not a MAC address")));
        }
        if let Some(id) = name.strip_prefix("node:") {
            return node_id(id).map(Self::Node);
        }
        if name == "logs" {
            return logs("");
        }
        if let Some(query) = name.strip_prefix("logs?")
            && !query.is_empty()
        {
            return logs(query);
        }
        if name == "events" {
            return events("");
        }
        if let Some(query) = name.strip_prefix("events?")
            && !query.is_empty()
        {
            return events(query);
        }
        Err(BadTopic(format!(
            "Unknown topic {name:?}: use fleet, node:<id>, logs?node=<id>&min=<severity>, \
             events?node=<id>&min=<level>, alerts, jobs, topology, clients, client:<mac> or rf"
        )))
    }
}

fn node_id(text: &str) -> Result<NodeId, BadTopic> {
    text.parse::<i64>()
        .ok()
        .filter(|id| *id > 0)
        .map(NodeId::new)
        .ok_or_else(|| BadTopic(format!("{text:?} is not a node ID")))
}

fn logs(query: &str) -> Result<Topic, BadTopic> {
    let (mut node, mut host, mut min) = (None, None, None);
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        match pair.split_once('=') {
            Some(("node", id)) if node.is_none() => node = Some(node_id(id)?),
            Some(("host", name)) if host.is_none() && valid_host(name) => {
                host = Some(name.to_owned());
            }
            Some(("min", severity)) if min.is_none() => {
                min = Some(Severity::parse(severity).ok_or_else(|| {
                    BadTopic(format!(
                        "{severity:?} is not a severity: use emergency, alert, critical, error, \
                         warning, notice, info or debug"
                    ))
                })?);
            }
            _ => return Err(BadTopic(format!("Unexpected {pair:?} in a logs topic"))),
        }
    }
    if node.is_some() && host.is_some() {
        return Err(BadTopic(
            "A logs topic names a node or a host, not both".to_owned(),
        ));
    }
    Ok(Topic::Logs {
        node,
        host,
        min: min.unwrap_or(Severity::Debug),
    })
}

fn events(query: &str) -> Result<Topic, BadTopic> {
    let (mut node, mut min) = (None, None);
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        match pair.split_once('=') {
            Some(("node", id)) if node.is_none() => node = Some(node_id(id)?),
            Some(("min", level)) if min.is_none() => {
                min = Some(EventLevel::parse(level).ok_or_else(|| {
                    BadTopic(format!(
                        "{level:?} is not a level: use critical, warning, notice or info"
                    ))
                })?);
            }
            _ => return Err(BadTopic(format!("Unexpected {pair:?} in an events topic"))),
        }
    }
    Ok(Topic::Events {
        node,
        min: min.unwrap_or(EventLevel::Info),
    })
}

/// A host's name or address, as the logs name other hosts.
fn valid_host(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-_:".contains(&b))
}

#[cfg(test)]
mod tests {
    use uniwrt_core::NodeId;
    use uniwrt_model::{events::EventLevel, log::Severity};

    use super::Topic;

    #[test]
    fn parses_each_kind() {
        assert_eq!(Topic::parse("fleet"), Ok(Topic::Fleet));
        assert_eq!(Topic::parse("topology"), Ok(Topic::Topology));
        assert_eq!(Topic::parse("clients"), Ok(Topic::Clients));
        assert_eq!(Topic::parse("alerts"), Ok(Topic::Alerts));
        assert_eq!(Topic::parse("rf"), Ok(Topic::Rf));
        assert_eq!(
            Topic::parse("client:F0:18:98:00:00:01"),
            Ok(Topic::Client("f0:18:98:00:00:01".parse().unwrap()))
        );
        assert_eq!(Topic::parse("node:7"), Ok(Topic::Node(NodeId::new(7))));
        let logs = |node: Option<i64>, host: Option<&str>, min| Topic::Logs {
            node: node.map(NodeId::new),
            host: host.map(str::to_owned),
            min,
        };
        let warning = logs(Some(3), None, Severity::Warning);
        assert_eq!(Topic::parse("logs?node=3&min=warning"), Ok(warning.clone()));
        assert_eq!(Topic::parse("logs?min=warning&node=3"), Ok(warning));
        assert_eq!(
            Topic::parse("logs?node=3"),
            Ok(logs(Some(3), None, Severity::Debug))
        );
        assert_eq!(Topic::parse("logs"), Ok(logs(None, None, Severity::Debug)));
        assert_eq!(
            Topic::parse("logs?min=err"),
            Ok(logs(None, None, Severity::Error))
        );
        assert_eq!(
            Topic::parse("logs?host=192.0.2.9"),
            Ok(logs(None, Some("192.0.2.9"), Severity::Debug))
        );
        let events = |node: Option<i64>, min| Topic::Events {
            node: node.map(NodeId::new),
            min,
        };
        assert_eq!(Topic::parse("events"), Ok(events(None, EventLevel::Info)));
        assert_eq!(
            Topic::parse("events?min=warning&node=2"),
            Ok(events(Some(2), EventLevel::Warning))
        );
    }

    #[test]
    fn refuses_what_it_does_not_know() {
        for bad in [
            "",
            "fleet ",
            "topology?",
            "node:",
            "node:0",
            "node:-1",
            "node:x",
            "logs?",
            "logs?node=3&host=sw",
            "logs?host=a/b",
            "logs?host=",
            "logs?min=info&min=debug",
            "logs?node=3&min=loud",
            "logs?node=3&node=4",
            "logs?node=3&level=info",
            "client:",
            "client:nope",
            "events?",
            "events?min=loud",
            "events?host=sw",
            "events?node=1&node=2",
        ] {
            assert!(Topic::parse(bad).is_err(), "{bad:?}");
        }
        assert!(Topic::parse(&format!("node:{}", "1".repeat(200))).is_err());
    }
}
