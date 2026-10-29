//! The live state: what the engine knows about each node now. The actors write it; readers
//! take copies (a status and an `Arc` of the facts), so no lock is held for long.

use std::{
    collections::BTreeMap,
    sync::{Arc, PoisonError, RwLock},
};

use uniwrt_core::NodeId;
use uniwrt_model::{facts::NodeFacts, status::NodeStatus};

/// One node, now.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeView {
    pub status: NodeStatus,
    /// The latest facts: fresh while the node is online, else the last ones seen (or stored);
    /// `None` until any arrive.
    pub facts: Option<Arc<NodeFacts>>,
}

#[derive(Debug, Default)]
pub(crate) struct Live(RwLock<BTreeMap<NodeId, NodeView>>);

impl Live {
    pub(crate) fn get(&self, node: NodeId) -> Option<NodeView> {
        self.read().get(&node).cloned()
    }

    pub(crate) fn all(&self) -> Vec<(NodeId, NodeView)> {
        self.read()
            .iter()
            .map(|(&node, view)| (node, view.clone()))
            .collect()
    }

    pub(crate) fn insert(&self, node: NodeId, view: NodeView) {
        self.write().insert(node, view);
    }

    pub(crate) fn remove(&self, node: NodeId) -> bool {
        self.write().remove(&node).is_some()
    }

    /// Changes a node that is watched; a removed one stays removed.
    pub(crate) fn update(&self, node: NodeId, update: impl FnOnce(&mut NodeView)) {
        if let Some(view) = self.write().get_mut(&node) {
            update(view);
        }
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, BTreeMap<NodeId, NodeView>> {
        self.0.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, BTreeMap<NodeId, NodeView>> {
        self.0.write().unwrap_or_else(PoisonError::into_inner)
    }
}
