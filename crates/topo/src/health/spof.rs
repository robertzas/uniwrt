//! Single points of failure: dominators from the gateways over the links that are up. Each
//! link is a vertex of its own between its ends, so one pass finds both what a vertex can't
//! lose (its nearest dominating vertex) and the links some relaying vertex can't lose.
//! Clients relay nothing: links lead into them, never out. The internet and the WAN are left
//! out (it is the LAN's paths to a gateway that count).
//!
//! The dominators are Cooper, Harvey and Kennedy's iterative ones ("A Simple, Fast Dominance
//! Algorithm"), which settle in a few passes on graphs like these.

use std::collections::BTreeMap;

use super::relays;
use crate::{Medium, TopoEdge, TopoKind, TopoNode};

/// Index 0 is a virtual root above every gateway; then the vertices, then the links.
const ROOT: usize = 0;

pub(super) fn find(nodes: &mut [TopoNode], edges: &mut [TopoEdge]) {
    let index: BTreeMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| node.kind != TopoKind::Internet)
        .map(|(at, node)| (node.id.as_str(), 1 + at))
        .collect();
    let link_base = 1 + nodes.len();
    let total = link_base + edges.len();
    let mut successors = vec![Vec::new(); total];
    let mut predecessors = vec![Vec::new(); total];
    let mut arc = |from: usize, to: usize| {
        successors[from].push(to);
        predecessors[to].push(from);
    };
    for (at, node) in nodes.iter().enumerate() {
        if node.kind == TopoKind::Gateway {
            arc(ROOT, 1 + at);
        }
    }
    for (at, edge) in edges.iter().enumerate() {
        let (Some(&a), Some(&b)) = (index.get(edge.a.as_str()), index.get(edge.b.as_str())) else {
            continue;
        };
        if !edge.up || edge.medium == Medium::Wan {
            continue;
        }
        let link = link_base + at;
        for (from, to) in [(a, b), (b, a)] {
            if relays(nodes[from - 1].kind) {
                arc(from, link);
            }
            arc(link, to);
        }
    }
    let idom = dominators(&successors, &predecessors);
    for at in 0..nodes.len() {
        nodes[at].depends_on = None;
        if !relays(nodes[at].kind) {
            continue;
        }
        let mut up = idom[1 + at];
        while let Some(dominator) = up.filter(|&dominator| dominator != ROOT) {
            if dominator >= link_base {
                edges[dominator - link_base].critical = true;
            } else if nodes[at].depends_on.is_none()
                && nodes[dominator - 1].kind != TopoKind::Gateway
            {
                nodes[at].depends_on = Some(nodes[dominator - 1].id.clone());
            }
            up = idom[dominator];
        }
    }
}

/// Each vertex's immediate dominator from [`ROOT`]; `None` for the root and for what it
/// doesn't reach.
fn dominators(successors: &[Vec<usize>], predecessors: &[Vec<usize>]) -> Vec<Option<usize>> {
    let order = postorder(successors);
    let mut rank = vec![usize::MAX; successors.len()];
    for (at, &vertex) in order.iter().enumerate() {
        rank[vertex] = at;
    }
    let mut idom = vec![None; successors.len()];
    idom[ROOT] = Some(ROOT);
    let mut changed = true;
    while changed {
        changed = false;
        for &vertex in order.iter().rev().skip(1) {
            let mut processed = predecessors[vertex]
                .iter()
                .copied()
                .filter(|&pred| idom[pred].is_some());
            let Some(first) = processed.next() else {
                continue;
            };
            let new = processed.fold(first, |new, pred| intersect(&idom, &rank, pred, new));
            if idom[vertex] != Some(new) {
                idom[vertex] = Some(new);
                changed = true;
            }
        }
    }
    idom[ROOT] = None;
    idom
}

fn intersect(idom: &[Option<usize>], rank: &[usize], mut a: usize, mut b: usize) -> usize {
    while a != b {
        while rank[a] < rank[b] {
            a = idom[a].unwrap_or(ROOT);
        }
        while rank[b] < rank[a] {
            b = idom[b].unwrap_or(ROOT);
        }
    }
    a
}

/// The vertices the root reaches, in postorder (the root last), without recursion.
fn postorder(successors: &[Vec<usize>]) -> Vec<usize> {
    let mut seen = vec![false; successors.len()];
    let mut order = Vec::new();
    let mut stack = vec![(ROOT, 0)];
    seen[ROOT] = true;
    while let Some((vertex, next)) = stack.last_mut() {
        if let Some(&child) = successors[*vertex].get(*next) {
            *next += 1;
            if !seen[child] {
                seen[child] = true;
                stack.push((child, 0));
            }
        } else {
            order.push(*vertex);
            stack.pop();
        }
    }
    order
}
