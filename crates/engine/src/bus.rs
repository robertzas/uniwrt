//! The event bus: what the engine announces as it happens, to the WS hub, the metrics writer,
//! the alert engine and anyone else subscribed ([`Engine::subscribe`](crate::Engine::subscribe)).

use std::sync::Arc;

use uniwrt_core::{NodeId, Timestamp};
use uniwrt_model::{
    event::{NodeEvent, Roam},
    facts::NodeFacts,
    log::LogEntry,
    status::NodeStatus,
};
use uniwrt_node::proto::ops::Set;

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
    /// The engine stopped watching a node.
    Removed { node: NodeId },
}

impl Event {
    /// The node it is about; `None` for a roam, which is about two.
    pub fn node(&self) -> Option<NodeId> {
        match self {
            Self::Status { node, .. }
            | Self::Facts { node, .. }
            | Self::Node { node, .. }
            | Self::Log { node, .. }
            | Self::Removed { node } => Some(*node),
            Self::Roam { .. } => None,
        }
    }
}
