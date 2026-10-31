//! A connection's subscriptions. Each topic answers with a snapshot when subscribed, then
//! gathers what the bus announces about it until the next [`flush`](Subscriptions::flush),
//! which turns it into at most one delta per topic: changes merge (a node's latest status, the
//! union of the facts' changed parts, the topology against the one the client has), while
//! events, roams and log entries queue up to a cap.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::Arc,
};

use tracing::warn;
use uniwrt_core::{JobId, NodeId};
use uniwrt_engine::{Event, Interest};
use uniwrt_model::{
    job::Job,
    log::{LogEntry, Severity},
    node::Node,
};
use uniwrt_node::proto::ops::Set;
use uniwrt_store::NodeRepo as _;
use uniwrt_topo::Topology;

use super::{
    message::{FactsPatch, ServerMessage, TimedEvent, TimedRoam},
    topic::{BadTopic, Topic},
};
use crate::{
    Api,
    jobs::DEFAULT_LIMIT,
    nodes::{NodeDetail, NodeSummary},
};

/// The most topics one connection may hold.
pub(crate) const MAX_TOPICS: usize = 32;
/// The most events (and roams) a node's topic keeps between flushes; older ones are dropped.
const MAX_EVENTS: usize = 256;
/// The most log entries a logs topic keeps between flushes; older ones are dropped and counted.
const MAX_LOG_ENTRIES: usize = 1_000;

#[derive(Default)]
pub(crate) struct Subscriptions {
    /// By the name the client gave, which every message about the topic carries.
    topics: BTreeMap<String, Sub>,
}

struct Sub {
    pending: Pending,
    /// Fast telemetry for a viewed node, or its log followed.
    _interest: Option<Interest>,
}

enum Pending {
    Fleet {
        /// The adopted nodes, read with the snapshot and as unknown ones show up.
        records: BTreeMap<NodeId, Node>,
        /// The summaries the client has, so that unchanged ones are not sent again.
        sent: BTreeMap<NodeId, NodeSummary>,
        dirty: BTreeSet<NodeId>,
        removed: BTreeSet<NodeId>,
    },
    Node {
        node: NodeId,
        status: bool,
        sets: BTreeSet<Set>,
        events: VecDeque<TimedEvent>,
        roams: VecDeque<TimedRoam>,
        removed: bool,
    },
    Logs {
        node: NodeId,
        min: Severity,
        entries: VecDeque<LogEntry>,
        dropped: u64,
    },
    Jobs {
        /// The latest state of each job changed since the last flush.
        changed: BTreeMap<JobId, Arc<Job>>,
    },
    Topology {
        /// The topology the client has, and the newest one announced since.
        sent: Arc<Topology>,
        latest: Option<Arc<Topology>>,
    },
}

impl Subscriptions {
    /// Makes `names` the subscribed topics; returns the new topics' snapshots and the errors.
    pub(crate) async fn set(&mut self, api: &Api, names: Vec<String>) -> Vec<ServerMessage> {
        let names: BTreeSet<String> = names.into_iter().collect();
        if names.len() > MAX_TOPICS {
            return vec![ServerMessage::Error {
                topic: None,
                message: format!("A connection subscribes to at most {MAX_TOPICS} topics"),
            }];
        }
        self.topics.retain(|name, _| names.contains(name));
        let mut out = Vec::new();
        for name in names {
            if self.topics.contains_key(&name) {
                continue;
            }
            match subscribe(api, &name).await {
                Ok((sub, snapshot)) => {
                    self.topics.insert(name, sub);
                    out.push(snapshot);
                }
                Err(BadTopic(message)) => out.push(ServerMessage::Error {
                    topic: Some(name),
                    message,
                }),
            }
        }
        out
    }

    /// Snapshots every topic again, as after missing events. A topic that fails (its node
    /// went) is dropped with an error.
    pub(crate) async fn resync(&mut self, api: &Api) -> Vec<ServerMessage> {
        let mut out = Vec::new();
        for name in self.topics.keys().cloned().collect::<Vec<_>>() {
            // The new interest is taken before the old one goes, so telemetry stays fast.
            match subscribe(api, &name).await {
                Ok((sub, snapshot)) => {
                    self.topics.insert(name, sub);
                    out.push(snapshot);
                }
                Err(BadTopic(message)) => {
                    self.topics.remove(&name);
                    out.push(ServerMessage::Error {
                        topic: Some(name),
                        message,
                    });
                }
            }
        }
        out
    }

    /// Notes what `event` changes for each topic.
    pub(crate) async fn apply(&mut self, api: &Api, event: &Event) {
        for sub in self.topics.values_mut() {
            sub.pending.apply(api, event).await;
        }
    }

    /// Notes a job's change for the `jobs` topic.
    pub(crate) fn job(&mut self, job: &Arc<Job>) {
        for sub in self.topics.values_mut() {
            if let Pending::Jobs { changed } = &mut sub.pending {
                changed.insert(job.id, Arc::clone(job));
            }
        }
    }

    /// One delta per topic with something to tell.
    pub(crate) fn flush(&mut self, api: &Api) -> Vec<ServerMessage> {
        self.topics
            .iter_mut()
            .filter_map(|(name, sub)| sub.pending.flush(api, name))
            .collect()
    }
}

/// Subscribes to the topic named `name`: its state and snapshot.
async fn subscribe(api: &Api, name: &str) -> Result<(Sub, ServerMessage), BadTopic> {
    let topic = name.to_owned();
    Ok(match Topic::parse(name)? {
        Topic::Fleet => {
            let records: BTreeMap<_, _> = api
                .store()
                .nodes()
                .await
                .map_err(|err| unavailable(&err))?
                .into_iter()
                .map(|node| (node.id, node))
                .collect();
            let views: BTreeMap<_, _> = api.fleet().all().into_iter().collect();
            let sent: BTreeMap<_, _> = records
                .values()
                .map(|node| (node.id, NodeSummary::new(node, views.get(&node.id))))
                .collect();
            let snapshot = ServerMessage::FleetSnapshot {
                topic,
                nodes: sent.values().cloned().collect(),
            };
            let pending = Pending::Fleet {
                records,
                sent,
                dirty: BTreeSet::new(),
                removed: BTreeSet::new(),
            };
            (sub(pending, None), snapshot)
        }
        Topic::Node(node) => {
            let record = read_node(api, node).await?;
            let interest = api.fleet().focus(node);
            let snapshot = ServerMessage::NodeSnapshot {
                topic,
                node: NodeDetail::new(record, api.fleet().node(node)),
            };
            let pending = Pending::Node {
                node,
                status: false,
                sets: BTreeSet::new(),
                events: VecDeque::new(),
                roams: VecDeque::new(),
                removed: false,
            };
            (sub(pending, interest), snapshot)
        }
        Topic::Logs { node, min } => {
            read_node(api, node).await?;
            let interest = api.fleet().follow_logs(node);
            let snapshot = ServerMessage::LogSnapshot {
                topic,
                entries: Vec::new(),
            };
            let pending = Pending::Logs {
                node,
                min,
                entries: VecDeque::new(),
                dropped: 0,
            };
            (sub(pending, interest), snapshot)
        }
        Topic::Jobs => {
            let jobs = api.jobs().jobs(DEFAULT_LIMIT).await.map_err(|err| {
                warn!(%err, "cannot read the jobs for a WS topic");
                BadTopic("The controller cannot read its jobs; its log has the details".to_owned())
            })?;
            let snapshot = ServerMessage::JobsSnapshot { topic, jobs };
            let pending = Pending::Jobs {
                changed: BTreeMap::new(),
            };
            (sub(pending, None), snapshot)
        }
        Topic::Topology => {
            let sent = api.fleet().topology();
            let snapshot = ServerMessage::TopologySnapshot {
                topic,
                topology: Arc::clone(&sent),
            };
            let pending = Pending::Topology { sent, latest: None };
            (sub(pending, None), snapshot)
        }
    })
}

const fn sub(pending: Pending, interest: Option<Interest>) -> Sub {
    Sub {
        pending,
        _interest: interest,
    }
}

async fn read_node(api: &Api, node: NodeId) -> Result<Node, BadTopic> {
    api.store()
        .node(node)
        .await
        .map_err(|err| unavailable(&err))?
        .ok_or_else(|| BadTopic(format!("There is no node {node}")))
}

fn unavailable(err: &uniwrt_store::Error) -> BadTopic {
    warn!(%err, "cannot read the nodes for a WS topic");
    BadTopic("The controller cannot read its nodes; its log has the details".to_owned())
}

impl Pending {
    async fn apply(&mut self, api: &Api, event: &Event) {
        match self {
            Self::Fleet {
                records,
                sent,
                dirty,
                removed,
            } => fleet_news(api, event, records, sent, dirty, removed).await,
            Self::Node {
                node,
                status,
                sets,
                events,
                roams,
                removed,
            } => match event {
                Event::Status { node: of, .. } if of == node => *status = true,
                Event::Facts {
                    node: of, changed, ..
                } if of == node => sets.extend(changed.iter().copied()),
                Event::Node {
                    node: of,
                    at,
                    reported_at,
                    event,
                } if of == node => push_capped(
                    events,
                    TimedEvent {
                        at: *at,
                        reported_at: *reported_at,
                        event: event.clone(),
                    },
                ),
                Event::Roam { at, roam } if roam.from.node == *node || roam.to.node == *node => {
                    push_capped(
                        roams,
                        TimedRoam {
                            at: *at,
                            roam: roam.clone(),
                        },
                    );
                }
                Event::Removed { node: of } if of == node => *removed = true,
                _ => {}
            },
            Self::Logs {
                node,
                min,
                entries,
                dropped,
            } => {
                if let Event::Log {
                    node: of,
                    entries: new,
                    dropped: lost,
                } = event
                    && of == node
                {
                    *dropped += lost;
                    entries.extend(new.iter().filter(|entry| entry.severity <= *min).cloned());
                    let excess = entries.len().saturating_sub(MAX_LOG_ENTRIES);
                    entries.drain(..excess);
                    *dropped += excess as u64;
                }
            }
            Self::Topology { latest, .. } => {
                if let Event::Topology { topology } = event {
                    *latest = Some(Arc::clone(topology));
                }
            }
            Self::Jobs { .. } => {}
        }
    }

    fn flush(&mut self, api: &Api, topic: &str) -> Option<ServerMessage> {
        let topic = topic.to_owned();
        match self {
            Self::Fleet {
                records,
                sent,
                dirty,
                removed,
            } => {
                let mut nodes = Vec::new();
                for node in std::mem::take(dirty) {
                    let Some(record) = records.get(&node) else {
                        continue;
                    };
                    let summary = NodeSummary::new(record, api.fleet().node(node).as_ref());
                    if sent.get(&node) != Some(&summary) {
                        sent.insert(node, summary.clone());
                        nodes.push(summary);
                    }
                }
                let removed: Vec<_> = std::mem::take(removed).into_iter().collect();
                (!nodes.is_empty() || !removed.is_empty()).then_some(ServerMessage::FleetDelta {
                    topic,
                    nodes,
                    removed,
                })
            }
            Self::Node {
                node,
                status,
                sets,
                events,
                roams,
                removed,
            } => {
                if !*status && sets.is_empty() && events.is_empty() && roams.is_empty() && !*removed
                {
                    return None;
                }
                let view = api.fleet().node(*node);
                let status = std::mem::take(status)
                    .then(|| view.as_ref().map(|view| view.status.clone()))
                    .flatten();
                let sets = std::mem::take(sets);
                let facts = view
                    .as_ref()
                    .and_then(|view| view.facts.as_ref())
                    .filter(|_| !sets.is_empty())
                    .map(|facts| Box::new(FactsPatch::new(facts, &sets)));
                Some(ServerMessage::NodeDelta {
                    topic,
                    status,
                    facts,
                    events: std::mem::take(events).into(),
                    roams: std::mem::take(roams).into(),
                    removed: std::mem::take(removed),
                })
            }
            Self::Logs {
                entries, dropped, ..
            } => (!entries.is_empty() || *dropped > 0).then(|| ServerMessage::LogDelta {
                topic,
                entries: std::mem::take(entries).into(),
                dropped: std::mem::take(dropped),
            }),
            Self::Jobs { changed } => (!changed.is_empty()).then(|| ServerMessage::JobsDelta {
                topic,
                jobs: std::mem::take(changed)
                    .into_values()
                    .map(|job| (*job).clone())
                    .collect(),
            }),
            Self::Topology { sent, latest } => {
                let latest = latest.take()?;
                let delta = sent.diff(&latest);
                *sent = latest;
                (!delta.is_empty()).then_some(ServerMessage::TopologyDelta { topic, delta })
            }
        }
    }
}

/// Notes what `event` changes for the `fleet` topic: a node that changed, or one adopted or
/// let go of since the snapshot.
async fn fleet_news(
    api: &Api,
    event: &Event,
    records: &mut BTreeMap<NodeId, Node>,
    sent: &mut BTreeMap<NodeId, NodeSummary>,
    dirty: &mut BTreeSet<NodeId>,
    removed: &mut BTreeSet<NodeId>,
) {
    let Some(node) = event.node() else { return };
    let known = records.contains_key(&node);
    if !known || matches!(event, Event::Removed { .. }) {
        // A node adopted since the snapshot, or one the engine let go of, which
        // may still be adopted (it is then shown as stopped).
        match api.store().node(node).await {
            Ok(Some(record)) => {
                records.insert(node, record);
            }
            Ok(None) => {
                records.remove(&node);
                dirty.remove(&node);
                if sent.remove(&node).is_some() {
                    removed.insert(node);
                }
                return;
            }
            Err(err) => {
                warn!(%err, node_id = %node, "cannot read a node for the fleet topic");
                return;
            }
        }
    }
    if matches!(
        event,
        Event::Status { .. } | Event::Facts { .. } | Event::Removed { .. }
    ) {
        removed.remove(&node);
        dirty.insert(node);
    }
}

fn push_capped<T>(queue: &mut VecDeque<T>, item: T) {
    if queue.len() == MAX_EVENTS {
        queue.pop_front();
    }
    queue.push_back(item);
}
