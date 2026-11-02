//! [`Deriver`]: what the engine announces, turned into events. Nodes here (status, reboots,
//! flash, WAN addresses), roams, log entries and jobs; links in `links.rs` and clients in
//! `clients.rs`.

use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    sync::Arc,
    time::Duration,
};

use uniwrt_clients::Client;
use uniwrt_core::{JobId, MacAddr, NodeId, Timestamp};
use uniwrt_model::{
    event::Roam,
    events::{EventCategory, EventKind, EventLevel, NewEvent},
    facts::NodeFacts,
    job::{Job, JobKind, JobState},
    log::{LogRecord, Severity},
    node::NodeRole,
    status::{NodeState, NodeStatus, StatusReason},
};

use crate::{
    DeriveConfig,
    clients::Seen,
    conditions::Conditions,
    links::{self, MeshLink},
    ms,
};

/// What the deriver knows when it starts.
#[derive(Debug, Default)]
pub struct Seed {
    /// When the deriver starts.
    pub started: Timestamp,
    /// The conditions that the store's latest events left raised, and when they were.
    pub raised: Vec<(String, Timestamp)>,
    /// Every client known, as they are now.
    pub clients: Vec<Arc<Client>>,
    /// The nodes' latest facts (restored from the store or live), with their roles.
    pub facts: Vec<(NodeId, NodeRole, Arc<NodeFacts>)>,
}

#[derive(Debug, Default)]
pub(crate) struct NodeEvents {
    pub(crate) role: Option<NodeRole>,
    /// Since when it is reachable, in this run.
    pub(crate) online_since: Option<Timestamp>,
    /// When it booted, and how long it had been up then, from its latest system facts.
    boot: Option<(Timestamp, u64)>,
    /// Each WAN interface's latest addresses and delegated prefixes, while it had any.
    wan: BTreeMap<String, Vec<String>>,
}

/// The jobs whose end was seen, the latest.
const JOBS_KEPT: usize = 64;

#[derive(Debug)]
pub struct Deriver {
    pub(crate) config: DeriveConfig,
    pub(crate) conditions: Conditions,
    pub(crate) nodes: BTreeMap<NodeId, NodeEvents>,
    pub(crate) clients: HashMap<MacAddr, Seen>,
    /// By the topology's edge id.
    pub(crate) mesh_links: BTreeMap<String, MeshLink>,
    ended: VecDeque<JobId>,
}

impl Deriver {
    pub fn new(config: DeriveConfig, seed: Seed) -> Self {
        let mut deriver = Self {
            config,
            conditions: Conditions::default(),
            nodes: BTreeMap::new(),
            clients: HashMap::new(),
            mesh_links: links::seeded(&seed.raised, seed.started),
            ended: VecDeque::new(),
        };
        for (key, at) in seed.raised {
            deriver.conditions.seed(key, at);
        }
        for client in &seed.clients {
            deriver.clients.insert(client.mac, Seen::of(client));
        }
        for (node, role, facts) in seed.facts {
            let state = deriver.nodes.entry(node).or_default();
            state.role = Some(role);
            if facts.system.uptime_s > 0 {
                state.boot = Some(boot(&facts));
            }
            state.wan = wan_addresses(role, &facts)
                .filter(|(_, addresses)| !addresses.is_empty())
                .collect();
        }
        deriver
    }

    /// A node's status changed.
    pub fn status(&mut self, node: NodeId, status: &NodeStatus, now: Timestamp) -> Vec<NewEvent> {
        let (offline, degraded) = (key(node, "offline"), key(node, "degraded"));
        let config = self.config;
        let state = self.nodes.entry(node).or_default();
        let conditions = &mut self.conditions;
        let back = |lasted: Option<Duration>| {
            NewEvent::new(
                now,
                EventKind::NodeOnline {
                    down_ms: lasted.map(ms),
                },
            )
            .node(node)
        };
        let mut events = Vec::new();
        match status.state {
            NodeState::Online | NodeState::Degraded => {
                state.online_since.get_or_insert(now);
                events.extend(conditions.good(&offline, now, back));
                if status.state == NodeState::Online {
                    events.extend(conditions.good(&degraded, now, |lasted| {
                        NewEvent::new(
                            now,
                            EventKind::NodeRecovered {
                                degraded_ms: lasted.map(ms),
                            },
                        )
                        .node(node)
                    }));
                } else {
                    let reasons = status.reasons.clone();
                    events.extend(conditions.bad(&degraded, now, config.degraded_after, || {
                        NewEvent::new(now, EventKind::NodeDegraded { reasons }).node(node)
                    }));
                }
            }
            NodeState::Offline => {
                state.online_since = None;
                conditions.unknown(&degraded);
                if status.reasons.contains(&StatusReason::Stopped) {
                    // The controller stopped watching it: nothing happened to the node.
                    conditions.unknown(&offline);
                } else {
                    let reasons = status.reasons.clone();
                    events.extend(conditions.bad(&offline, now, config.offline_after, || {
                        NewEvent::new(now, EventKind::NodeOffline { reasons }).node(node)
                    }));
                }
            }
        }
        events
    }

    /// A node's facts changed; `system_fresh` when its system set was just collected (else
    /// its uptime and storage are older than `collected_at`).
    pub fn facts(
        &mut self,
        node: NodeId,
        role: NodeRole,
        facts: &NodeFacts,
        system_fresh: bool,
        now: Timestamp,
    ) -> Vec<NewEvent> {
        let config = self.config;
        let state = self.nodes.entry(node).or_default();
        state.role = Some(role);
        let mut events = Vec::new();
        if system_fresh && facts.system.uptime_s > 0 {
            let (booted, up_s) = boot(facts);
            if let Some((before, up_before)) = state.boot
                && booted > before.saturating_add(config.reboot_slack)
            {
                // How long it had been up when last seen before the reboot.
                let up_s = Some(up_before);
                events.push(
                    NewEvent::new(booted.min(now), EventKind::NodeRebooted { up_s }).node(node),
                );
            }
            state.boot = Some((booted, up_s));
        }
        if system_fresh
            && let Some(root) = &facts.system.root
            && root.total_bytes > 0
        {
            #[allow(clippy::cast_precision_loss)]
            let free = root.available_bytes as f64 / root.total_bytes as f64;
            let (available_bytes, total_bytes) = (root.available_bytes, root.total_bytes);
            let flash = key(node, "flash");
            if free < config.flash_low_below {
                events.extend(self.conditions.bad(&flash, now, Duration::ZERO, || {
                    NewEvent::new(
                        now,
                        EventKind::FlashLow {
                            available_bytes,
                            total_bytes,
                        },
                    )
                    .node(node)
                }));
            } else if free >= config.flash_fine_from {
                events.extend(self.conditions.good(&flash, now, |_| {
                    NewEvent::new(
                        now,
                        EventKind::FlashOk {
                            available_bytes,
                            total_bytes,
                        },
                    )
                    .node(node)
                }));
            }
        }
        for (interface, after) in wan_addresses(role, facts).collect::<Vec<_>>() {
            if after.is_empty() {
                continue;
            }
            let before = state.wan.insert(interface.clone(), after.clone());
            if let Some(before) = before
                && before != after
            {
                events.push(
                    NewEvent::new(
                        now,
                        EventKind::WanAddressChanged {
                            interface,
                            before,
                            after,
                        },
                    )
                    .node(node),
                );
            }
        }
        events
    }

    /// The engine stopped watching a node: its conditions and state go, without events.
    pub fn removed(&mut self, node: NodeId) {
        self.nodes.remove(&node);
        self.conditions.forget(&format!("node:{node}/"));
    }

    /// Conditions whose bad state has lasted long enough by `now`.
    pub fn tick(&mut self, now: Timestamp) -> Vec<NewEvent> {
        self.conditions.tick(now)
    }

    /// A Wi-Fi client roamed at `at`.
    pub fn roam(&self, roam: &Roam, at: Timestamp) -> NewEvent {
        let fast = roam.auth_alg.as_deref() == Some("ft");
        NewEvent::new(
            at,
            EventKind::ClientRoamed {
                from: roam.from.clone(),
                to: roam.to.clone(),
                fast,
            },
        )
        .node(roam.to.node)
        .client(roam.mac)
    }

    /// Log entries as stored: those whose pattern is an event (`event_of` tells its category
    /// and severity) are events, a source's repeats of one pattern folding together.
    pub fn logged(
        &self,
        records: &[LogRecord],
        event_of: impl Fn(&str) -> Option<(EventCategory, Severity)>,
    ) -> Vec<NewEvent> {
        let mut events = Vec::new();
        for record in records {
            let Some(pattern) = &record.pattern else {
                continue;
            };
            let Some((category, severity)) = event_of(pattern) else {
                continue;
            };
            let source = match (record.node, &record.host) {
                (Some(node), _) => format!("node:{node}"),
                (None, Some(host)) => format!("host:{host}"),
                (None, None) => "unknown".to_owned(),
            };
            let entry = &record.entry;
            let mut event = NewEvent::new(
                record.received,
                EventKind::LogPattern {
                    pattern: pattern.clone(),
                    log: record.id,
                    host: record.host.clone().filter(|_| record.node.is_none()),
                    program: entry.program.clone(),
                    text: cut(&entry.message, self.config.max_text),
                },
            )
            .fold(format!("log:{pattern}:{source}"));
            event.category = category;
            event.level = EventLevel::from_severity(severity);
            event.node = record.node;
            events.push(event);
        }
        events
    }

    /// A job changed: an adoption that succeeded and a job that failed are events, once.
    pub fn job(&mut self, job: &Job, now: Timestamp) -> Option<NewEvent> {
        if job.state == JobState::Running || self.ended.contains(&job.id) {
            return None;
        }
        if self.ended.len() == JOBS_KEPT {
            self.ended.pop_front();
        }
        self.ended.push_back(job.id);
        let at = job.finished_at.unwrap_or(now);
        let event = match (job.state, job.kind) {
            (JobState::Succeeded, JobKind::Adopt) => NewEvent::new(at, EventKind::NodeAdopted),
            (JobState::Failed, kind) => NewEvent::new(
                at,
                EventKind::JobFailed {
                    job: job.id,
                    job_kind: kind,
                    error: job.error.as_ref().map(|error| error.message.clone()),
                },
            ),
            _ => return None,
        };
        Some(match job.node {
            Some(node) => event.node(node),
            None => event,
        })
    }
}

/// A node's condition key, such as `node:3/offline`.
pub(crate) fn key(node: NodeId, what: &str) -> String {
    format!("node:{node}/{what}")
}

/// When a node booted, from fresh system facts, and how long it has been up.
fn boot(facts: &NodeFacts) -> (Timestamp, u64) {
    let up = facts.system.uptime_s;
    (
        facts.collected_at.saturating_sub(Duration::from_secs(up)),
        up,
    )
}

/// A gateway's WAN interfaces and their IPv4 addresses and delegated prefixes.
fn wan_addresses(
    role: NodeRole,
    facts: &NodeFacts,
) -> impl Iterator<Item = (String, Vec<String>)> + '_ {
    facts
        .interfaces
        .iter()
        .filter(move |iface| role == NodeRole::Gateway && uniwrt_topo::is_wan(iface))
        .map(|iface| {
            let v4 = iface.ipv4.iter().map(ToString::to_string);
            let prefixes = iface.delegated_prefixes.iter().map(ToString::to_string);
            (iface.name.clone(), v4.chain(prefixes).collect())
        })
}

/// `text` cut to at most `max` bytes, at a character boundary, with an ellipsis.
fn cut(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let end = (0..=max)
        .rev()
        .find(|&i| text.is_char_boundary(i))
        .unwrap_or(0);
    format!("{}…", &text[..end])
}
