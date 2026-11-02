//! Links from topology builds: gateways' WAN links (down while they have no default route) and
//! mesh links between adopted nodes, down (reported down, or gone from the topology while both
//! nodes are online: batman-adv forgets a lost neighbour) or weak.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    time::Duration,
};

use uniwrt_core::{NodeId, Timestamp};
use uniwrt_model::events::{EventKind, NewEvent};
use uniwrt_topo::{Medium, TopoEdge, Topology};

use crate::{Deriver, derive::key, ms};

/// How long a mesh link may stay gone before it is forgotten (the mesh was rewired).
pub(crate) const FORGET_LINK: Duration = Duration::from_hours(24);

/// A mesh link between adopted nodes: its downstream node, its upstream peer, and when the
/// topology last had it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MeshLink {
    pub(crate) node: NodeId,
    pub(crate) peer: NodeId,
    pub(crate) seen: Timestamp,
}

impl MeshLink {
    /// The link a condition key names (`node:<b>/mesh:<edge>`), from the edge id's ends.
    pub(crate) fn from_key(key: &str, at: Timestamp) -> Option<(String, Self)> {
        let (node, edge) = key.split_once("/mesh:")?;
        let node = node_id(node)?;
        let mut ends = edge.split('~').take(2).map(node_id);
        let (Some(Some(a)), Some(Some(b))) = (ends.next(), ends.next()) else {
            return None;
        };
        let peer = if a == node { b } else { a };
        Some((
            edge.to_owned(),
            Self {
                node,
                peer,
                seen: at,
            },
        ))
    }
}

fn node_id(vertex: &str) -> Option<NodeId> {
    let id = vertex.strip_prefix("node:")?.parse::<i64>().ok()?;
    (id > 0).then(|| NodeId::new(id))
}

impl Deriver {
    /// A topology build at `now`.
    pub fn topology(&mut self, topology: &Topology, now: Timestamp) -> Vec<NewEvent> {
        let nodes: HashMap<&str, NodeId> = topology
            .nodes
            .iter()
            .filter_map(|vertex| Some((vertex.id.as_str(), vertex.node?)))
            .collect();
        let mut events = Vec::new();
        let mut present = BTreeSet::new();
        for edge in &topology.edges {
            match edge.medium {
                Medium::Wan => events.extend(self.wan(edge, &nodes, now)),
                Medium::Mesh => {
                    let (Some(&a), Some(&b)) =
                        (nodes.get(edge.a.as_str()), nodes.get(edge.b.as_str()))
                    else {
                        continue;
                    };
                    present.insert(edge.id.clone());
                    let link = MeshLink {
                        node: b,
                        peer: a,
                        seen: now,
                    };
                    self.mesh_links.insert(edge.id.clone(), link);
                    events.extend(self.mesh(&edge.id, link, Some(edge), now));
                }
                Medium::Ethernet | Medium::Wifi => {}
            }
        }
        let gone: Vec<(String, MeshLink)> = self
            .mesh_links
            .iter()
            .filter(|(id, _)| !present.contains(*id))
            .map(|(id, link)| (id.clone(), *link))
            .collect();
        for (id, link) in gone {
            if now.saturating_duration_since(link.seen) > FORGET_LINK {
                self.mesh_links.remove(&id);
                self.conditions
                    .forget(&key(link.node, &format!("mesh:{id}")));
                self.conditions
                    .forget(&key(link.node, &format!("mesh-weak:{id}")));
            } else {
                events.extend(self.mesh(&id, link, None, now));
            }
        }
        events
    }

    fn wan(
        &mut self,
        edge: &TopoEdge,
        nodes: &HashMap<&str, NodeId>,
        now: Timestamp,
    ) -> Option<NewEvent> {
        let (node, port) = match (nodes.get(edge.b.as_str()), nodes.get(edge.a.as_str())) {
            (Some(&node), _) => (node, &edge.b_port),
            (None, Some(&node)) => (node, &edge.a_port),
            (None, None) => return None,
        };
        let device = port.clone().unwrap_or_else(|| edge.id.clone());
        let key = key(node, &format!("wan:{device}"));
        if !self.is_online(node) {
            self.conditions.unknown(&key);
            return None;
        }
        if edge.up {
            self.conditions.good(&key, now, |lasted| {
                NewEvent::new(
                    now,
                    EventKind::WanUp {
                        device: device.clone(),
                        down_ms: lasted.map(ms),
                    },
                )
                .node(node)
            })
        } else {
            let after = self.config.wan_down_after;
            self.conditions.bad(&key, now, after, || {
                NewEvent::new(
                    now,
                    EventKind::WanDown {
                        device: device.clone(),
                    },
                )
                .node(node)
            })
        }
    }

    /// A mesh link as the build has it (`edge`), or gone from it (`None`).
    fn mesh(
        &mut self,
        id: &str,
        link: MeshLink,
        edge: Option<&TopoEdge>,
        now: Timestamp,
    ) -> Vec<NewEvent> {
        let MeshLink { node, peer, .. } = link;
        let (down, weak) = (
            key(node, &format!("mesh:{id}")),
            key(node, &format!("mesh-weak:{id}")),
        );
        if !self.is_online(node) || !self.is_online(peer) {
            self.conditions.unknown(&down);
            self.conditions.unknown(&weak);
            return Vec::new();
        }
        let config = self.config;
        let link_id = id.to_owned();
        let mut events = Vec::new();
        let Some(edge) = edge.filter(|edge| edge.up) else {
            self.conditions.unknown(&weak);
            events.extend(self.conditions.bad(&down, now, config.mesh_down_after, || {
                NewEvent::new(
                    now,
                    EventKind::MeshLinkDown {
                        link: link_id,
                        peer,
                    },
                )
                .node(node)
            }));
            return events;
        };
        events.extend(self.conditions.good(&down, now, |lasted| {
            NewEvent::new(
                now,
                EventKind::MeshLinkUp {
                    link: link_id.clone(),
                    peer,
                    down_ms: lasted.map(ms),
                },
            )
            .node(node)
        }));
        match edge.quality {
            Some(quality) if quality < config.mesh_weak_below => {
                events.extend(self.conditions.bad(&weak, now, config.mesh_weak_after, || {
                    NewEvent::new(
                        now,
                        EventKind::MeshLinkWeak {
                            link: link_id.clone(),
                            peer,
                            quality,
                        },
                    )
                    .node(node)
                }));
            }
            Some(quality) if quality >= config.mesh_fine_from => {
                events.extend(self.conditions.good(&weak, now, |lasted| {
                    NewEvent::new(
                        now,
                        EventKind::MeshLinkRecovered {
                            link: link_id.clone(),
                            peer,
                            quality,
                            weak_ms: lasted.map(ms),
                        },
                    )
                    .node(node)
                }));
            }
            _ => {}
        }
        events
    }
}

/// The mesh links that seeded conditions name, so that a link down when the controller stopped
/// is still known (as seen when the deriver started, `started`).
pub(crate) fn seeded(
    raised: &[(String, Timestamp)],
    started: Timestamp,
) -> BTreeMap<String, MeshLink> {
    raised
        .iter()
        .filter_map(|(key, _)| MeshLink::from_key(key, started))
        .collect()
}
