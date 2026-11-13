//! What a revision does to each node's mesh (features/mesh.md, Mesh settings; M5.5), from the
//! packages it holds and the ones rendered (pure): whether its 802.11s mesh point comes, moves
//! (another mesh ID, key, band or channel, or another routing algorithm) or goes, and whether
//! batman-adv goes with it. [`review`] warns of the nodes a change cuts off for a while, and
//! refuses the apply where it would leave a node with no path to the gateway.

use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

use uniwrt_core::NodeId;
use uniwrt_model::apply::{NodePlan, PlanAction, PlanIssue};
use uniwrt_uci::Package;

/// What a revision does to one node's mesh.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum MeshChange {
    /// No mesh before or after, or the same one.
    #[default]
    Same,
    /// A mesh point or batman-adv comes where there was none: no link drops.
    Joins,
    /// The mesh point's links drop until its peers have the change too.
    Moves,
    /// The mesh point goes; batman-adv stays (over cables).
    Leaves,
    /// batman-adv goes, and the mesh point with it if there was one.
    Off,
}

impl MeshChange {
    /// The node loses the mesh's Wi-Fi for good.
    pub(crate) fn grounds(self) -> bool {
        matches!(self, Self::Leaves | Self::Off)
    }

    /// Links over the mesh drop, for a while or for good.
    fn cuts(self) -> bool {
        matches!(self, Self::Moves | Self::Leaves | Self::Off)
    }
}

/// The settings two mesh points must share to link.
#[derive(Debug, PartialEq, Eq)]
struct Point<'a> {
    mesh_id: Option<&'a str>,
    key: Option<&'a str>,
    band: Option<&'a str>,
    channel: Option<&'a str>,
}

/// The enabled mesh point a `wireless` package holds, on an enabled radio.
fn point(wireless: Option<&Package>) -> Option<Point<'_>> {
    let wireless = wireless?;
    let on = |section: &uniwrt_uci::Section| section.option("disabled") != Some("1");
    let iface = wireless
        .sections_of("wifi-iface")
        .find(|s| s.option("mode") == Some("mesh") && on(s))?;
    let radio = iface
        .option("device")
        .and_then(|name| wireless.section(name));
    if radio.is_some_and(|radio| !on(radio)) {
        return None;
    }
    Some(Point {
        mesh_id: iface.option("mesh_id"),
        key: iface.option("key"),
        band: radio.and_then(|r| r.option("band")),
        channel: radio.and_then(|r| r.option("channel")),
    })
}

/// The routing algorithm of the batman-adv interface a `network` package holds.
fn routing(network: Option<&Package>) -> Option<&str> {
    network?
        .sections_of("interface")
        .find(|s| s.option("proto") == Some("batadv"))
        // batman-adv's own default.
        .map(|s| s.option("routing_algo").unwrap_or("BATMAN_IV"))
}

/// What the rendered `network` and `wireless` do to the held ones' mesh.
pub(crate) fn change(
    held: (Option<&Package>, Option<&Package>),
    next: (Option<&Package>, Option<&Package>),
) -> MeshChange {
    let (was, will) = (routing(held.0), routing(next.0));
    let (from, to) = (point(held.1), point(next.1));
    if was.is_some() && will.is_none() {
        return MeshChange::Off;
    }
    match (from, to) {
        (Some(_), None) => MeshChange::Leaves,
        (Some(a), Some(b)) if a != b => MeshChange::Moves,
        _ if was.is_some() && will.is_some() && was != will => MeshChange::Moves,
        (None, Some(_)) => MeshChange::Joins,
        _ if was.is_none() && will.is_some() => MeshChange::Joins,
        _ => MeshChange::Same,
    }
}

/// Where some node's mesh links drop: the plans of the nodes in `air` (they reach the gateway
/// over the mesh's Wi-Fi alone) are refused if they leave it, else warned that they drop off
/// until the mesh comes back within `window`; nodes in observe mode, which keep their mesh,
/// are warned too.
pub(crate) fn review(
    plans: &mut [NodePlan],
    changes: &BTreeMap<NodeId, MeshChange>,
    air: &BTreeSet<NodeId>,
    window: Duration,
) {
    if !changes.values().any(|change| change.cuts()) {
        return;
    }
    let note = |message: String| PlanIssue {
        path: "mesh".to_owned(),
        message,
    };
    for plan in plans {
        match plan.action {
            PlanAction::Skipped => plan.warnings.push(note(
                "In observe mode it keeps the mesh's current settings: if it reaches the network \
                 over the mesh's Wi-Fi, it loses it. Turn management on first, so that it \
                 changes with the others"
                    .to_owned(),
            )),
            PlanAction::Change | PlanAction::Unchanged if air.contains(&plan.node) => {
                if changes
                    .get(&plan.node)
                    .is_some_and(|change| change.grounds())
                {
                    plan.issues.push(note(
                        "It reaches the gateway over the mesh's Wi-Fi alone: without its mesh \
                         point it would lose the network, and the apply would roll back. \
                         Connect it by cable first"
                            .to_owned(),
                    ));
                    plan.action = PlanAction::Refused;
                } else {
                    plan.warnings.push(note(format!(
                        "It reaches the gateway over the mesh's Wi-Fi alone, so it drops off \
                         while the mesh changes, until the nodes on its way have the change \
                         too. The apply waits up to {} s for the mesh to come back, and rolls \
                         every node back if it doesn't",
                        window.as_secs()
                    )));
                }
            }
            PlanAction::Refused | PlanAction::Change | PlanAction::Unchanged => {}
        }
    }
}

#[cfg(test)]
#[path = "mesh_change_tests.rs"]
mod tests;
