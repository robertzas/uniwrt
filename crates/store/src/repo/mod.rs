//! Repository traits: the store's API by concern. `Store` implements each; services depend on the
//! traits, so their tests can substitute fakes.

mod facts;
mod nodes;

use uniwrt_core::NodeId;
use uniwrt_model::{
    facts::NodeFacts,
    node::{NewNode, Node},
};

use crate::Result;

/// The node inventory.
pub trait NodeRepo: Send + Sync {
    /// Adds a node in observe mode. A node with the same MAC is an
    /// [`Error::Conflict`](crate::Error::Conflict).
    fn create_node(&self, node: NewNode) -> impl Future<Output = Result<Node>> + Send;

    fn node(&self, id: NodeId) -> impl Future<Output = Result<Option<Node>>> + Send;

    /// All nodes, oldest first.
    fn nodes(&self) -> impl Future<Output = Result<Vec<Node>>> + Send;

    /// Removes a node and everything stored about it; false when there was no such node.
    fn delete_node(&self, id: NodeId) -> impl Future<Output = Result<bool>> + Send;
}

/// The latest facts snapshot of each node.
pub trait FactsRepo: Send + Sync {
    /// Replaces the node's snapshot, unless the stored one was collected later. An unknown node
    /// is an [`Error::NotFound`](crate::Error::NotFound).
    fn save_facts(
        &self,
        node: NodeId,
        facts: &NodeFacts,
    ) -> impl Future<Output = Result<()>> + Send;

    fn latest_facts(&self, node: NodeId) -> impl Future<Output = Result<Option<NodeFacts>>> + Send;
}
