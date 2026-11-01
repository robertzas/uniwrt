//! [`Fleet`]: the engine as its readers see it, whatever the link type, so the API holds one
//! `Arc<dyn Fleet>` while the server runs `Engine<FleetLink>` (D-041).

use std::sync::Arc;

use tokio::sync::broadcast;
use uniwrt_clients::{Client, Detail};
use uniwrt_core::{MacAddr, NodeId};
use uniwrt_model::client::ClientRecord;
use uniwrt_node::NodeLink;
use uniwrt_topo::Topology;

use crate::{Engine, Event, Interest, NodeView};

/// The engine's link-free face: the live state, the topology, the clients, the bus and
/// interest.
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
    /// See [`Engine::topology`].
    fn topology(&self) -> Arc<Topology>;
    /// See [`Engine::clients`].
    fn clients(&self) -> Vec<Arc<Client>>;
    /// See [`Engine::client`].
    fn client(&self, mac: MacAddr) -> Option<Detail>;
    /// See [`Engine::rename_client`].
    fn rename_client(&self, mac: MacAddr, alias: Option<String>) -> Option<ClientRecord>;
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

    fn topology(&self) -> Arc<Topology> {
        Self::topology(self)
    }

    fn clients(&self) -> Vec<Arc<Client>> {
        Self::clients(self)
    }

    fn client(&self, mac: MacAddr) -> Option<Detail> {
        Self::client(self, mac)
    }

    fn rename_client(&self, mac: MacAddr, alias: Option<String>) -> Option<ClientRecord> {
        Self::rename_client(self, mac, alias)
    }
}
