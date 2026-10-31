//! The topology: the network graph that the adopted nodes' facts describe.
//!
//! - **API:** [`build`] takes the adopted nodes as [`Member`]s (id, role, adoption MAC, status
//!   and latest facts) and the previous build, and returns a [`Topology`]: [`TopoNode`]s (the
//!   internet, the nodes, inferred switches, mesh peers that are not adopted, clients) and
//!   [`TopoEdge`]s (WAN, Ethernet, mesh and Wi-Fi links, each with its ports, quality, capacity
//!   and [`Traffic`]). [`Topology::diff`] and [`Topology::apply`] turn one build into the next
//!   with a [`TopologyDelta`].
//! - **How** (features/observability.md, Topology): a gateway's interfaces with a default route
//!   (or named `wan*`) link it to the internet; Wi-Fi stations place clients at their access
//!   point (the most recently active one while a client roams) and link a bridge to its access
//!   point; 802.11s peer links and batman-adv neighbors are mesh links over Wi-Fi and wired ones
//!   over Ethernet; the bridges' forwarding databases give the remaining wired links, inferring
//!   an unmanaged switch where several nodes or clients share a segment (`wired.rs`);
//!   batman-adv's translation tables, then the neighbor caches, place the clients left. The
//!   tree view follows the cheapest paths from the internet (or the gateway), where batman-adv's
//!   best paths toward the gateway, wires and the WAN are cheap; edges point downstream.
//! - **Invariants:** pure and deterministic: no IO, no clock (times come from the facts); the
//!   same input gives the same graph, sorted by id. Ids are stable across builds (vertices by
//!   node id or MAC, edges by their ends, medium and band), so a diff carries only what changed.
//!   Every client has one vertex and one edge. Links reported by both ends are one edge; its
//!   traffic comes from the same end's counters build after build (the gateway's when it is
//!   one), and its rates from the previous build's reading.
//! - **Dependencies:** `uniwrt-core`, `uniwrt-model`, serde; ts-rs behind the `ts` feature.
//! - **Tests:** scenarios built in code next to each concept (`tests/`), and the engine's tests
//!   over the hwsim lab's captures and the simulated fleet (`cargo nextest run -p uniwrt-topo`).

mod clients;
mod diff;
mod fleet;
mod graph;
mod links;
mod mesh;
mod quality;
mod tree;
mod wan;
mod wifi;
mod wired;

#[cfg(test)]
mod tests;

use std::net::IpAddr;

use uniwrt_model::{facts::NodeFacts, node::NodeRole};

use self::{
    clients::Directory,
    fleet::{Fleet, node_vertex},
    links::Graph,
};
pub use self::{
    diff::TopologyDelta,
    fleet::Member,
    graph::{Medium, TopoEdge, TopoKind, TopoNode, Topology, Traffic},
};

/// The topology of the members' network. `previous` is the last build, for traffic rates.
pub fn build(members: &[Member<'_>], previous: Option<&Topology>) -> Topology {
    let fleet = Fleet::new(members);
    let directory = Directory::new(&fleet);
    let mut graph = Graph::default();
    for member in &fleet.members {
        graph.vertex(vertex(member));
    }
    wan::link(&mut graph, &fleet);
    wifi::place(&mut graph, &fleet, &directory);
    mesh::link(&mut graph, &fleet, &directory);
    mesh::place(&mut graph, &fleet, &directory, false);
    wired::link(&mut graph, &fleet, &directory);
    mesh::place(&mut graph, &fleet, &directory, true);
    clients::place_neighbors(&mut graph, &fleet, &directory);
    let at = fleet
        .with_facts()
        .map(|(_, _, facts)| facts.collected_at)
        .max()
        .unwrap_or_default();
    graph.finish(at, previous)
}

fn vertex(member: &Member<'_>) -> TopoNode {
    let facts = member.facts;
    TopoNode {
        id: node_vertex(member.id),
        kind: match member.role {
            NodeRole::Gateway => TopoKind::Gateway,
            NodeRole::Ap => TopoKind::Ap,
            NodeRole::Bridge => TopoKind::Bridge,
        },
        node: Some(member.id),
        mac: Some(member.mac),
        name: facts.map(|facts| facts.board.hostname.clone()),
        ips: facts.map(addresses).unwrap_or_default(),
        state: Some(member.state),
        cpu_percent: facts.and_then(|facts| facts.system.cpu_percent),
        memory_percent: facts.and_then(memory_percent),
        parent: None,
        uplink: None,
        hops: None,
    }
}

/// The node's addresses, but its loopback's.
fn addresses(facts: &NodeFacts) -> Vec<IpAddr> {
    facts
        .interfaces
        .iter()
        .filter(|iface| iface.name != "loopback")
        .flat_map(|iface| {
            let v4 = iface.ipv4.iter().map(|net| IpAddr::V4(net.addr()));
            let v6 = iface.ipv6.iter().map(|net| IpAddr::V6(net.addr()));
            v4.chain(v6)
        })
        .collect()
}

#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)] // A share, 0–100.
fn memory_percent(facts: &NodeFacts) -> Option<f32> {
    let memory = &facts.system.memory;
    let used = memory.total_bytes.checked_sub(memory.available_bytes)?;
    (memory.total_bytes > 0).then(|| (used as f64 * 100.0 / memory.total_bytes as f64) as f32)
}
