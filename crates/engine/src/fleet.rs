//! [`Fleet`]: the engine as its readers see it, whatever the link type, so the API holds one
//! `Arc<dyn Fleet>` while the server runs `Engine<FleetLink>` (D-041).

use tokio::sync::broadcast;
use uniwrt_core::NodeId;
use uniwrt_node::NodeLink;

use crate::{Engine, Event, Interest, NodeView};

/// The engine's link-free face: the live state, the bus and interest.
pub trait Fleet: Send + Sync + 'static {
    /// See [`Engine::all`].
    fn all(&self) -> Vec<(NodeId, NodeView)>;
    /// See [`Engine::node`].
    fn node(&self, node: NodeId) -> Option<NodeView>;
    /// See [`Engine::focus`].
    fn focus(&self, node: NodeId) -> Option<Interest>;
    /// See [`Engine::follow_logs`].
    fn follow_logs(&self, node: NodeId) -> Option<Interest>;
    /// See [`Engine::subscribe`].
    fn subscribe(&self) -> broadcast::Receiver<Event>;
}

impl<L: NodeLink> Fleet for Engine<L> {
    fn all(&self) -> Vec<(NodeId, NodeView)> {
        Self::all(self)
    }

    fn node(&self, node: NodeId) -> Option<NodeView> {
        Self::node(self, node)
    }

    fn focus(&self, node: NodeId) -> Option<Interest> {
        Self::focus(self, node)
    }

    fn follow_logs(&self, node: NodeId) -> Option<Interest> {
        Self::follow_logs(self, node)
    }

    fn subscribe(&self) -> broadcast::Receiver<Event> {
        Self::subscribe(self)
    }
}
