//! The order an apply goes through its nodes: leaves → gateway (features/lifecycle.md, Plan), by
//! wired and mesh hops from the gateway in the topology the nodes' facts describe, so that no
//! node's change cuts the controller off from one not applied yet.

use std::collections::{BTreeMap, VecDeque};

use uniwrt_core::NodeId;
use uniwrt_model::{
    apply::{NodePlan, PlanAction},
    facts::NodeFacts,
    node::{Node, NodeRole},
    status::NodeState,
};
use uniwrt_topo::{Member, TopoKind, build};

/// Hops from the gateway to each node the topology connects to it, counting the nodes on the
/// way: links run between nodes and through inferred switches, which cost nothing (a node
/// behind one is as near as one on the gateway's port), never through clients or the internet.
pub(crate) fn hops(nodes: &[(&Node, &NodeFacts)]) -> BTreeMap<NodeId, u32> {
    let members: Vec<Member<'_>> = nodes
        .iter()
        .map(|(node, facts)| Member {
            id: node.id,
            role: node.role,
            mac: node.mac,
            state: NodeState::Online,
            facts: Some(facts),
        })
        .collect();
    let topology = build(&members, None);
    // The vertices a path may cross, with the node each one is (`None` for a switch).
    let infrastructure: BTreeMap<&str, Option<NodeId>> = topology
        .nodes
        .iter()
        .filter(|vertex| {
            matches!(
                vertex.kind,
                TopoKind::Gateway | TopoKind::Ap | TopoKind::Bridge | TopoKind::Switch
            )
        })
        .map(|vertex| (vertex.id.as_str(), vertex.node))
        .collect();
    let Some(start) = topology
        .nodes
        .iter()
        .find(|vertex| vertex.kind == TopoKind::Gateway)
    else {
        return BTreeMap::new();
    };
    // A 0-1 breadth-first search: free steps go to the front, so the queue stays by cost.
    let mut best = BTreeMap::from([(start.id.as_str(), 0_u32)]);
    let mut queue = VecDeque::from([(start.id.as_str(), 0_u32)]);
    while let Some((at, cost)) = queue.pop_front() {
        if best.get(at).is_some_and(|&known| known < cost) {
            continue;
        }
        for edge in &topology.edges {
            let other = if edge.a == at {
                edge.b.as_str()
            } else if edge.b == at {
                edge.a.as_str()
            } else {
                continue;
            };
            let Some(owner) = infrastructure.get(other) else {
                continue;
            };
            let step = cost + u32::from(owner.is_some());
            if best.get(other).is_none_or(|&known| step < known) {
                best.insert(other, step);
                if owner.is_some() {
                    queue.push_back((other, step));
                } else {
                    queue.push_front((other, step));
                }
            }
        }
    }
    best.into_iter()
        .filter_map(|(id, cost)| Some((infrastructure.get(id).copied().flatten()?, cost)))
        .collect()
}

/// Sorts plans into the order they apply: the farthest first (unknown distance counts as
/// farthest), the gateway last, then the nodes left alone; by id among equals.
pub(crate) fn sort(plans: &mut [NodePlan]) {
    plans.sort_by_key(|plan| {
        (
            plan.action == PlanAction::Skipped,
            plan.role == NodeRole::Gateway,
            std::cmp::Reverse(plan.hops.unwrap_or(u32::MAX)),
            plan.node,
        )
    });
}

#[cfg(test)]
mod tests {
    use uniwrt_core::NodeId;
    use uniwrt_model::{
        apply::{NodePlan, PlanAction},
        node::NodeRole,
    };

    use super::sort;

    fn plan(id: i64, role: NodeRole, hops: Option<u32>, action: PlanAction) -> NodePlan {
        NodePlan {
            node: NodeId::new(id),
            name: format!("n{id}"),
            role,
            action,
            reason: None,
            packages: Vec::new(),
            raw: Vec::new(),
            services: Vec::new(),
            address: None,
            hops,
            timeout_s: 120,
            warnings: Vec::new(),
            issues: Vec::new(),
        }
    }

    #[test]
    fn leaves_first_the_gateway_last_and_skipped_nodes_after() {
        let mut plans = vec![
            plan(1, NodeRole::Gateway, Some(0), PlanAction::Change),
            plan(2, NodeRole::Ap, Some(1), PlanAction::Change),
            plan(3, NodeRole::Ap, Some(2), PlanAction::Unchanged),
            plan(4, NodeRole::Ap, None, PlanAction::Change),
            plan(5, NodeRole::Ap, Some(2), PlanAction::Skipped),
            plan(6, NodeRole::Ap, Some(1), PlanAction::Change),
        ];
        sort(&mut plans);
        let order: Vec<i64> = plans.iter().map(|plan| plan.node.get()).collect();
        assert_eq!(order, [4, 3, 2, 6, 1, 5]);
    }
}
