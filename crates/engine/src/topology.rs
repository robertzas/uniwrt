//! The fleet's topology (`uniwrt_topo`): rebuilt from the live state a moment after a node's
//! facts or status change (the changes of that moment share one build), kept for readers, and
//! announced as [`Event::Topology`] when it changed; each build updates the clients' book
//! ([`Event::Clients`]). A [`LinkWatch`] sees every build, for the backhaul links' drops.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, PoisonError, RwLock},
};

use tokio::sync::Notify;
use uniwrt_core::{MacAddr, NodeId};
use uniwrt_model::node::{Node, NodeRole};
use uniwrt_topo::{LinkWatch, Member, Topology};

use crate::{Event, engine::Context};

#[cfg(test)]
#[path = "topology_tests.rs"]
mod tests;

/// Who a node is in the network, which its facts don't say: what it was adopted as and by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Identity {
    pub role: NodeRole,
    pub mac: MacAddr,
}

impl From<&Node> for Identity {
    fn from(node: &Node) -> Self {
        Self {
            role: node.role,
            mac: node.mac,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct TopologyState {
    current: RwLock<Arc<Topology>>,
    identities: Mutex<BTreeMap<NodeId, Identity>>,
    /// The backhaul links' drops across builds.
    watch: Mutex<LinkWatch>,
    changed: Notify,
}

impl TopologyState {
    pub(crate) fn current(&self) -> Arc<Topology> {
        Arc::clone(&self.current.read().unwrap_or_else(PoisonError::into_inner))
    }

    pub(crate) fn identify(&self, node: NodeId, identity: Identity) {
        self.identities().insert(node, identity);
        self.changed();
    }

    /// The role a node was adopted in.
    pub(crate) fn role(&self, node: NodeId) -> Option<NodeRole> {
        self.identities().get(&node).map(|identity| identity.role)
    }

    pub(crate) fn forget(&self, node: NodeId) {
        self.identities().remove(&node);
        self.changed();
    }

    /// Something the topology is built from changed: a build follows shortly.
    pub(crate) fn changed(&self) {
        self.changed.notify_one();
    }

    fn identities(&self) -> std::sync::MutexGuard<'_, BTreeMap<NodeId, Identity>> {
        self.identities
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

/// Builds the topology after each change, a moment later, until the engine stops.
pub(crate) async fn run(ctx: Arc<Context>) {
    loop {
        ctx.topology.changed.notified().await;
        tokio::time::sleep(ctx.config.topology_delay).await;
        rebuild(&ctx);
    }
}

fn rebuild(ctx: &Context) {
    let views = ctx.live.all();
    let identities = ctx.topology.identities().clone();
    let members: Vec<Member<'_>> = views
        .iter()
        .filter_map(|(node, view)| {
            let identity = identities.get(node)?;
            Some(Member {
                id: *node,
                role: identity.role,
                mac: identity.mac,
                state: view.status.state,
                facts: view.facts.as_deref(),
            })
        })
        .collect();
    let previous = ctx.topology.current();
    let mut topology = uniwrt_topo::build(&members, Some(&previous));
    ctx.topology
        .watch
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .observe(&mut topology);
    let topology = Arc::new(topology);
    let changed = topology.nodes != previous.nodes || topology.edges != previous.edges;
    *ctx.topology
        .current
        .write()
        .unwrap_or_else(PoisonError::into_inner) = Arc::clone(&topology);
    let clients = ctx.clients().update(&topology, &members);
    if changed {
        ctx.publish(Event::Topology { topology });
    }
    ctx.publish_clients(clients);
}
