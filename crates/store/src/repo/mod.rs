//! Repository traits: the store's API by concern. `Store` implements each; services depend on the
//! traits, so their tests can substitute fakes.

mod facts;
mod host_keys;
mod nodes;

use uniwrt_core::NodeId;
use uniwrt_model::{
    facts::NodeFacts,
    node::{HostKeyPin, NewNode, Node},
};
use zeroize::Zeroizing;

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

/// The SSH host key pinned for each node.
pub trait HostKeyRepo: Send + Sync {
    /// Pins the node's host key, replacing the previous pin (an admin re-trusts a reflashed
    /// node). An unknown node is an [`Error::NotFound`](crate::Error::NotFound).
    fn pin_host_key(
        &self,
        node: NodeId,
        pin: HostKeyPin,
    ) -> impl Future<Output = Result<()>> + Send;

    fn host_key(&self, node: NodeId) -> impl Future<Output = Result<Option<HostKeyPin>>> + Send;
}

/// A secret's bytes, wiped from memory when dropped.
pub type Secret = Zeroizing<Vec<u8>>;

/// Secrets by name, such as `controller/ssh_key`, sealed at rest ([`Vault`](crate::Vault)).
pub trait VaultRepo: Send + Sync {
    /// Stores a secret, replacing any with the same name.
    fn put_secret(&self, name: &str, value: Secret) -> impl Future<Output = Result<()>> + Send;

    fn secret(&self, name: &str) -> impl Future<Output = Result<Option<Secret>>> + Send;

    /// Removes a secret; false when there was no such secret.
    fn delete_secret(&self, name: &str) -> impl Future<Output = Result<bool>> + Send;
}
