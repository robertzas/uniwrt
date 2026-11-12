//! Drops across builds: a backhaul link that goes down, or vanishes, while both its ends are
//! online. One build knows only its own links, so whoever keeps the topology keeps a
//! [`LinkWatch`] beside it and shows it every build.

use std::collections::{BTreeMap, BTreeSet};

use uniwrt_core::Timestamp;
use uniwrt_model::status::NodeState;

use crate::{TopoKind, Topology, health::rescore};

/// How long a drop counts: two in this window make a link flapping.
pub const DROP_WINDOW_MS: i64 = 30 * 60 * 1000;

/// The backhaul links seen lately: whether each was up, and its drops.
#[derive(Debug, Default)]
pub struct LinkWatch {
    pub(crate) links: BTreeMap<String, Watched>,
}

#[derive(Debug)]
pub(crate) struct Watched {
    ends: [String; 2],
    up: bool,
    drops: Vec<Timestamp>,
}

impl LinkWatch {
    /// Notes what changed since the last build, and gives the build's backhaul links their
    /// drops and the score that follows. A link that vanished counts as a drop when both its
    /// ends are still there and online (a node that went away took its links with it).
    pub fn observe(&mut self, topology: &mut Topology) {
        let at = topology.at;
        let online: BTreeSet<&str> = topology
            .nodes
            .iter()
            .filter(|node| {
                node.kind != TopoKind::Client && node.state.is_none_or(|s| s == NodeState::Online)
            })
            .map(|node| node.id.as_str())
            .collect();
        let both_online = |ends: &[String; 2]| ends.iter().all(|end| online.contains(end.as_str()));
        let since = Timestamp::from_unix_ms(at.as_unix_ms().saturating_sub(DROP_WINDOW_MS));
        let mut seen = BTreeSet::new();
        for edge in topology
            .edges
            .iter_mut()
            .filter(|edge| edge.score.is_some())
        {
            seen.insert(edge.id.clone());
            let ends = [edge.a.clone(), edge.b.clone()];
            let watched = self.links.entry(edge.id.clone()).or_insert(Watched {
                ends: ends.clone(),
                up: edge.up,
                drops: Vec::new(),
            });
            if watched.up && !edge.up && both_online(&ends) {
                watched.drops.push(at);
            }
            watched.up = edge.up;
            watched.ends = ends;
            watched.drops.retain(|drop| *drop > since);
            edge.drops.clone_from(&watched.drops);
            rescore(edge);
        }
        self.links.retain(|id, watched| {
            if seen.contains(id) {
                return true;
            }
            if watched.up && both_online(&watched.ends) {
                watched.drops.push(at);
            }
            watched.up = false;
            watched.drops.retain(|drop| *drop > since);
            !watched.drops.is_empty()
        });
    }
}
