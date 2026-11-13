//! The simulated fleet as a brownfield (features/lifecycle.md, Brownfield import): each node's
//! facts, as its agent reports them, and its `/etc/config` before uniwrt manages it
//! (`render::config`), for import's round-trip tests and the demo.

use std::collections::BTreeMap;

use uniwrt_core::Timestamp;
use uniwrt_model::{facts::NodeFacts, node::NodeRole};
use uniwrt_node::{
    facts,
    proto::ops::{Collected, Set},
};

use crate::{
    SimConfig,
    render::{collect, commands::Disk, config},
    world::World,
};

/// One node of the fleet, as found.
#[derive(Debug, Clone)]
pub struct Brownfield {
    pub name: String,
    pub role: NodeRole,
    pub facts: NodeFacts,
    /// Its UCI packages, by name, as text.
    pub files: BTreeMap<String, String>,
}

/// The fleet's nodes, the gateway first, at `start`.
///
/// # Panics
///
/// If the simulated agent's answer doesn't parse: the sim's own tests forbid that.
pub fn brownfield(config: &SimConfig, start: Timestamp) -> Vec<Brownfield> {
    of(&World::new(config, start))
}

/// The nodes of `world` as found, the gateway first; the bench device is no part of it.
pub(crate) fn of(world: &World) -> Vec<Brownfield> {
    (0..world.nodes.len())
        .filter(|&n| !world.is_bench(n))
        .map(|n| {
            let mut disk = Disk::default();
            disk.tool.files = config::files(world, n);
            let collected: Collected = serde_json::from_value(collect(world, n, &Set::ALL, &disk))
                .expect("the sim's collect parses");
            let mut found = NodeFacts::default();
            let _ = facts::apply(&collected, &mut found);
            let node = &world.nodes[n];
            Brownfield {
                name: node.name.clone(),
                role: if node.gateway {
                    NodeRole::Gateway
                } else {
                    NodeRole::Ap
                },
                facts: found,
                files: disk.tool.files,
            }
        })
        .collect()
}
