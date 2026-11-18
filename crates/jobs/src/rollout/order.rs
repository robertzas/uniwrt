//! The order a rollout upgrades its nodes in: leaves first (the most hops from the gateway in
//! the topology every adopted node's facts describe, as an apply goes), the gateway last, the
//! canary first of all (D-125).

use std::collections::BTreeMap;

use uniwrt_core::NodeId;
use uniwrt_model::{
    facts::NodeFacts,
    node::{Node, NodeRole},
};

use crate::apply::order;

/// `chosen` in upgrade order, by every adopted node with its facts (the whole topology, not the
/// chosen nodes alone); `canary` first when given, else the first in order. Why not, when the
/// canary is the gateway of a rollout of several.
pub(crate) fn sort(
    adopted: &[(Node, Option<NodeFacts>)],
    chosen: &[NodeId],
    canary: Option<NodeId>,
) -> Result<Vec<NodeId>, String> {
    let known: Vec<(&Node, &NodeFacts)> = adopted
        .iter()
        .filter_map(|(node, facts)| Some((node, facts.as_ref()?)))
        .collect();
    let hops = order::hops(&order::topology(&known));
    let roles: BTreeMap<NodeId, NodeRole> = adopted
        .iter()
        .map(|(node, _)| (node.id, node.role))
        .collect();
    let mut sorted = chosen.to_vec();
    sorted.sort_by_key(|id| {
        (
            roles.get(id) == Some(&NodeRole::Gateway),
            std::cmp::Reverse(hops.get(id).copied().unwrap_or(u32::MAX)),
            *id,
        )
    });
    if let Some(canary) = canary {
        if sorted.len() > 1 && roles.get(&canary) == Some(&NodeRole::Gateway) {
            return Err(
                "The gateway can't be the canary: it upgrades last, after every other node"
                    .to_owned(),
            );
        }
        let Some(at) = sorted.iter().position(|&id| id == canary) else {
            return Err(format!("The canary, node {canary}, isn't in the rollout"));
        };
        let canary = sorted.remove(at);
        sorted.insert(0, canary);
    }
    Ok(sorted)
}
