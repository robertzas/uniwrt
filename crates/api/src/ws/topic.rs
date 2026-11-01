//! WS topics as clients name them: `fleet`, `node:<id>`, `logs?node=<id>&min=<severity>` (`min`
//! is optional: every entry by default; the parameters may come in any order), `jobs`,
//! `topology`, `clients` and `client:<mac>`.

use std::fmt;

use uniwrt_core::{MacAddr, NodeId};
use uniwrt_model::log::Severity;

/// The longest topic name accepted.
pub(crate) const MAX_LEN: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Topic {
    /// Every node's summary.
    Fleet,
    /// One node: its status, facts, events and roams; viewing it speeds up its telemetry.
    Node(NodeId),
    /// One node's log entries at `min` or more severe.
    Logs { node: NodeId, min: Severity },
    /// The latest jobs, and every change to a job.
    Jobs,
    /// The fleet's topology, then what changes in it.
    Topology,
    /// Every client, then the ones that change.
    Clients,
    /// One client: its view, samples and sessions; viewing it speeds up its node's telemetry.
    Client(MacAddr),
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
        if name == "topology" {
            return Ok(Self::Topology);
        }
        if name == "clients" {
            return Ok(Self::Clients);
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
        if let Some(query) = name.strip_prefix("logs?") {
            return logs(query);
        }
        Err(BadTopic(format!(
            "Unknown topic {name:?}: use fleet, node:<id>, logs?node=<id>&min=<severity>, jobs, \
             topology, clients or client:<mac>"
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
    let mut node = None;
    let mut min = Severity::Debug;
    for pair in query.split('&') {
        match pair.split_once('=') {
            Some(("node", id)) if node.is_none() => node = Some(node_id(id)?),
            Some(("min", severity)) => {
                min = serde_json::from_value(serde_json::Value::String(severity.to_owned()))
                    .map_err(|_| {
                        BadTopic(format!(
                            "{severity:?} is not a severity: use emergency, alert, critical, \
                             error, warning, notice, info or debug"
                        ))
                    })?;
            }
            _ => return Err(BadTopic(format!("Unexpected {pair:?} in a logs topic"))),
        }
    }
    let node = node.ok_or_else(|| BadTopic("A logs topic names its node=<id>".to_owned()))?;
    Ok(Topic::Logs { node, min })
}

#[cfg(test)]
mod tests {
    use uniwrt_core::NodeId;
    use uniwrt_model::log::Severity;

    use super::Topic;

    #[test]
    fn parses_each_kind() {
        assert_eq!(Topic::parse("fleet"), Ok(Topic::Fleet));
        assert_eq!(Topic::parse("topology"), Ok(Topic::Topology));
        assert_eq!(Topic::parse("clients"), Ok(Topic::Clients));
        assert_eq!(
            Topic::parse("client:F0:18:98:00:00:01"),
            Ok(Topic::Client("f0:18:98:00:00:01".parse().unwrap()))
        );
        assert_eq!(Topic::parse("node:7"), Ok(Topic::Node(NodeId::new(7))));
        let logs = Topic::Logs {
            node: NodeId::new(3),
            min: Severity::Warning,
        };
        assert_eq!(Topic::parse("logs?node=3&min=warning"), Ok(logs));
        assert_eq!(Topic::parse("logs?min=warning&node=3"), Ok(logs));
        assert_eq!(
            Topic::parse("logs?node=3"),
            Ok(Topic::Logs {
                node: NodeId::new(3),
                min: Severity::Debug
            })
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
            "logs?min=warning",
            "logs?node=3&min=loud",
            "logs?node=3&node=4",
            "logs?node=3&level=info",
            "client:",
            "client:nope",
        ] {
            assert!(Topic::parse(bad).is_err(), "{bad:?}");
        }
        assert!(Topic::parse(&format!("node:{}", "1".repeat(200))).is_err());
    }
}
