//! The event bus: what the engine announces as it happens, to the WS hub, the metrics writer,
//! the alert engine and anyone else subscribed ([`Engine::subscribe`](crate::Engine::subscribe)).

use std::sync::Arc;

use uniwrt_clients::Changes;
use uniwrt_core::{NodeId, Timestamp};
use uniwrt_model::{
    event::{NodeEvent, Roam},
    facts::NodeFacts,
    log::{LogEntry, LogRecord},
    status::NodeStatus,
};
use uniwrt_node::proto::ops::Set;
use uniwrt_topo::Topology;

/// One announcement. Clones are cheap: facts and log batches are shared.
#[derive(Debug, Clone)]
pub enum Event {
    /// A node's status changed.
    Status { node: NodeId, status: NodeStatus },
    /// A node's facts changed; `changed` names the sets whose parts differ from before.
    Facts {
        node: NodeId,
        facts: Arc<NodeFacts>,
        changed: Vec<Set>,
    },
    /// Something a node reported as it happened.
    Node {
        node: NodeId,
        /// When it arrived, on the controller's clock.
        at: Timestamp,
        /// When it happened, on the node's clock (which may be skewed).
        reported_at: Option<Timestamp>,
        event: NodeEvent,
    },
    /// A Wi-Fi client moved to another access point.
    Roam { at: Timestamp, roam: Roam },
    /// Entries of a node's log, oldest first, while someone follows it
    /// ([`Engine::follow_logs`](crate::Engine::follow_logs)); `dropped` counts the entries lost
    /// before them.
    Log {
        node: NodeId,
        entries: Arc<[LogEntry]>,
        dropped: u64,
    },
    /// Log entries as stored ([`LogRecorder`](crate::LogRecorder)), from nodes and other
    /// hosts, in the order they were stored.
    Logged { records: Arc<[LogRecord]> },
    /// The engine stopped watching a node.
    Removed { node: NodeId },
    /// The topology changed ([`Engine::topology`](crate::Engine::topology)).
    Topology { topology: Arc<Topology> },
    /// Clients changed ([`Engine::clients`](crate::Engine::clients)): their views, the records
    /// and sessions to keep, and new samples.
    Clients { changes: Arc<Changes> },
}

impl Event {
    /// The node it is about; `None` for a roam, which is about two, the topology, clients and
    /// stored log entries.
    pub fn node(&self) -> Option<NodeId> {
        match self {
            Self::Status { node, .. }
            | Self::Facts { node, .. }
            | Self::Node { node, .. }
            | Self::Log { node, .. }
            | Self::Removed { node } => Some(*node),
            Self::Roam { .. }
            | Self::Topology { .. }
            | Self::Clients { .. }
            | Self::Logged { .. } => None,
        }
    }
}
