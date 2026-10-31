//! What changed between two builds, by id: the WS topic sends it instead of the whole graph.

use serde::{Deserialize, Serialize};
use uniwrt_core::Timestamp;

use crate::{TopoEdge, TopoNode, Topology};

/// The vertices and edges that are new or changed (whole) and the ids of those that went.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct TopologyDelta {
    pub at: Timestamp,
    pub nodes: Vec<TopoNode>,
    pub edges: Vec<TopoEdge>,
    pub removed_nodes: Vec<String>,
    pub removed_edges: Vec<String>,
}

impl TopologyDelta {
    /// Nothing but the time changed.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
            && self.edges.is_empty()
            && self.removed_nodes.is_empty()
            && self.removed_edges.is_empty()
    }
}

impl Topology {
    /// What turns this topology into `newer`.
    pub fn diff(&self, newer: &Self) -> TopologyDelta {
        let (nodes, removed_nodes) = changes(&self.nodes, &newer.nodes, |node| &node.id);
        let (edges, removed_edges) = changes(&self.edges, &newer.edges, |edge| &edge.id);
        TopologyDelta {
            at: newer.at,
            nodes,
            edges,
            removed_nodes,
            removed_edges,
        }
    }

    /// Applies a delta, keeping both lists sorted by id.
    pub fn apply(&mut self, delta: &TopologyDelta) {
        self.at = delta.at;
        patch(
            &mut self.nodes,
            &delta.nodes,
            &delta.removed_nodes,
            |node| &node.id,
        );
        patch(
            &mut self.edges,
            &delta.edges,
            &delta.removed_edges,
            |edge| &edge.id,
        );
    }
}

/// The items of `new` that `old` lacks or has otherwise, and the ids of those `new` lacks; both
/// lists are sorted by id.
fn changes<T: Clone + PartialEq>(
    old: &[T],
    new: &[T],
    id: impl Fn(&T) -> &String,
) -> (Vec<T>, Vec<String>) {
    let (mut changed, mut removed) = (Vec::new(), Vec::new());
    let (mut old, mut new) = (old.iter().peekable(), new.iter().peekable());
    loop {
        match (old.peek(), new.peek()) {
            (None, None) => break,
            (Some(before), Some(after)) if id(before) == id(after) => {
                if before != after {
                    changed.push((*after).clone());
                }
                old.next();
                new.next();
            }
            (Some(before), Some(after)) if id(before) < id(after) => {
                removed.push(id(before).clone());
                old.next();
            }
            (Some(before), None) => {
                removed.push(id(before).clone());
                old.next();
            }
            (_, Some(after)) => {
                changed.push((*after).clone());
                new.next();
            }
        }
    }
    (changed, removed)
}

fn patch<T: Clone>(
    items: &mut Vec<T>,
    upserts: &[T],
    removed: &[String],
    id: impl Fn(&T) -> &String,
) {
    items.retain(|item| removed.binary_search(id(item)).is_err());
    for upsert in upserts {
        match items.binary_search_by(|item| id(item).cmp(id(upsert))) {
            Ok(index) => items[index] = upsert.clone(),
            Err(index) => items.insert(index, upsert.clone()),
        }
    }
}
