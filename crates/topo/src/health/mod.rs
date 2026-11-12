//! The mesh's health (features/mesh.md, Telemetry & health), worked out on a finished build:
//! each link's score and its weakest factor, each vertex's hops to its gateway, and the single
//! points of failure ([`spof`]). Drops come from [`LinkWatch`](crate::LinkWatch), which
//! scores again with them.

mod spof;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use crate::{LinkFactor, Medium, TopoEdge, TopoKind, TopoNode, Topology};

impl Topology {
    /// Scores, hops to the gateway and single points of failure, from the build's links.
    pub(crate) fn assess(&mut self) {
        let kinds: BTreeMap<&str, TopoKind> = self
            .nodes
            .iter()
            .map(|node| (node.id.as_str(), node.kind))
            .collect();
        for edge in &mut self.edges {
            let backhaul = [&edge.a, &edge.b]
                .iter()
                .all(|end| kinds.get(end.as_str()).is_some_and(|kind| relays(*kind)));
            if backhaul && edge.medium != Medium::Wan {
                rescore(edge);
            }
        }
        gateway_hops(&mut self.nodes, &self.edges);
        spof::find(&mut self.nodes, &mut self.edges);
    }
}

/// Whether a vertex can carry other vertices' traffic: not the internet, not a client.
pub(crate) const fn relays(kind: TopoKind) -> bool {
    !matches!(kind, TopoKind::Internet | TopoKind::Client)
}

/// Sets a backhaul link's score and weakest factor from what it measures and its drops.
pub(crate) fn rescore(edge: &mut TopoEdge) {
    let (score, weakest) = score(edge);
    edge.score = Some(score);
    edge.weakest = (score < 100).then_some(weakest).flatten();
}

/// The lowest of the link's factors, and which it is (the first in [`LinkFactor`]'s order on a
/// tie); 100 and none when nothing measured holds it back.
fn score(edge: &TopoEdge) -> (u8, Option<LinkFactor>) {
    let air = matches!(edge.medium, Medium::Mesh | Medium::Wifi);
    let drops = u8::try_from(edge.drops.len()).unwrap_or(u8::MAX);
    let factors = [
        (LinkFactor::Down, (!edge.up).then_some(0)),
        (
            LinkFactor::Flapping,
            (drops >= 2).then(|| 100u8.saturating_sub(30u8.saturating_mul(drops - 1))),
        ),
        (
            LinkFactor::Signal,
            air.then(|| edge.signal_dbm.map(crate::quality::signal_quality))
                .flatten(),
        ),
        (LinkFactor::Tq, edge.tq.map(crate::quality::tq_quality)),
        (
            LinkFactor::Throughput,
            edge.capacity_mbps
                .filter(|mbps| air || *mbps < 100)
                .map(throughput_score),
        ),
    ];
    factors
        .into_iter()
        .filter_map(|(factor, value)| Some((value?, factor)))
        .min_by_key(|(value, _)| *value)
        .map_or((100, None), |(value, factor)| (value, Some(factor)))
}

/// 0 at 10 Mbit/s and below, 100 at 300 and above, on a log scale between (100 Mbit/s is 68,
/// 50 is 47).
fn throughput_score(mbps: u32) -> u8 {
    const LOW: f64 = 10.0;
    const HIGH: f64 = 300.0;
    let share = (f64::from(mbps).max(LOW) / LOW).ln() / (HIGH / LOW).ln();
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // 0–100.
    let score = (share.min(1.0) * 100.0).round() as u8;
    score
}

/// Each relaying vertex's links to its gateway along the tree view: an edge counts unless its
/// upper end is a switch (the cable through it counts once), and over the air as well when it
/// is a mesh or Wi-Fi link.
fn gateway_hops(nodes: &mut [TopoNode], edges: &[TopoEdge]) {
    let index: BTreeMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(at, node)| (node.id.as_str(), at))
        .collect();
    let media: BTreeMap<&str, Medium> = edges
        .iter()
        .map(|edge| (edge.id.as_str(), edge.medium))
        .collect();
    let mut hops = vec![None; nodes.len()];
    for start in 0..nodes.len() {
        if !relays(nodes[start].kind) {
            continue;
        }
        let (mut all, mut air, mut at) = (0u16, 0u16, start);
        hops[start] = loop {
            let node = &nodes[at];
            if node.kind == TopoKind::Gateway {
                break Some((all, air));
            }
            let (Some(parent), Some(uplink)) = (node.parent.as_deref(), node.uplink.as_deref())
            else {
                break None;
            };
            let Some(&next) = index.get(parent) else {
                break None;
            };
            if nodes[next].kind != TopoKind::Switch {
                all = all.saturating_add(1);
            }
            if matches!(media.get(uplink), Some(Medium::Mesh | Medium::Wifi)) {
                air = air.saturating_add(1);
            }
            at = next;
        };
    }
    for (node, hops) in nodes.iter_mut().zip(hops) {
        (node.gateway_hops, node.wireless_hops) = hops.unzip();
    }
}
