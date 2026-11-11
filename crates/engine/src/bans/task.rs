//! The bans' task: which nodes to send what, kicks, and what to announce.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};

use tokio::{
    sync::{
        broadcast::{self, error::RecvError},
        mpsc, oneshot,
    },
    task::JoinSet,
};
use tracing::{debug, warn};
use uniwrt_clients::Changes;
use uniwrt_core::{JobId, MacAddr, NodeId, RevisionId, Timestamp};
use uniwrt_model::{
    client::ClientMedium,
    events::{EventKind, NewEvent},
    job::{Job, JobState},
    site::{SiteConfig, node::ManagedPackage},
    status::NodeState,
};
use uniwrt_node::{
    Error as LinkError, NodeLink,
    proto::ops::{Ban, BanEntry, Banned},
};
use uniwrt_store::RevisionRepo;

use super::{
    BansConfig, KickFailure, Kicked, KickedFrom,
    plan::{self, Held, Terms, Wanted},
};
use crate::{Engine, Event, wlans::next_job};

pub(super) enum Command {
    Kick {
        mac: MacAddr,
        ban: Duration,
        by: Option<String>,
        reply: oneshot::Sender<Kicked>,
    },
}

/// What the controller believes a node holds, and how its requests went.
#[derive(Default)]
struct NodeBans {
    held: Held,
    /// When it was last sent its bans.
    sent: Option<Timestamp>,
    /// Requests running now.
    running: u32,
    /// Whether its latest request failed (its condition is raised).
    failing: bool,
}

/// A `ban` request that ended.
struct Done {
    node: NodeId,
    at: Timestamp,
    entries: Vec<BanEntry>,
    result: Result<Banned, String>,
}

pub(super) struct Task<L, R> {
    engine: Engine<L>,
    repo: R,
    config: BansConfig,
    head: Option<(RevisionId, Arc<SiteConfig>)>,
    /// The nodes each running job holds.
    jobs: BTreeMap<JobId, Vec<NodeId>>,
    nodes: BTreeMap<NodeId, NodeBans>,
    blocked: BTreeSet<MacAddr>,
    /// Kicked clients, until when.
    kicked: BTreeMap<MacAddr, Timestamp>,
    done: (mpsc::Sender<Done>, mpsc::Receiver<Done>),
}

impl<L, R> Task<L, R>
where
    L: NodeLink,
    R: RevisionRepo + Clone + 'static,
{
    pub(super) fn new(engine: Engine<L>, repo: R, config: BansConfig) -> Self {
        Self {
            engine,
            repo,
            config,
            head: None,
            jobs: BTreeMap::new(),
            nodes: BTreeMap::new(),
            blocked: BTreeSet::new(),
            kicked: BTreeMap::new(),
            done: mpsc::channel(32),
        }
    }

    pub(super) async fn run(
        mut self,
        mut bus: broadcast::Receiver<Event>,
        mut jobs: Option<broadcast::Receiver<Arc<Job>>>,
        mut commands: mpsc::Receiver<Command>,
        mut stopped: oneshot::Receiver<()>,
    ) {
        let mut tick = tokio::time::interval(self.config.tick_every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                biased;
                _ = &mut stopped => break,
                Some(command) = commands.recv() => match command {
                    Command::Kick { mac, ban, by, reply } => {
                        self.reload().await;
                        self.kick(mac, ban, by, reply);
                    }
                },
                Some(job) = next_job(&mut jobs) => self.on_job(&job),
                Some(done) = self.done.1.recv() => self.on_done(done),
                event = bus.recv() => match event {
                    Ok(Event::Status { node, status }) => {
                        if status.state == NodeState::Offline {
                            // Back, it may have restarted: it gets every ban again.
                            if let Some(bans) = self.nodes.get_mut(&node) {
                                bans.held.clear();
                            }
                        } else {
                            self.compare(Some(node), false);
                        }
                    }
                    Ok(Event::Removed { node }) => {
                        self.nodes.remove(&node);
                    }
                    Ok(Event::Clients { changes }) => self.on_clients(&changes),
                    Ok(_) | Err(RecvError::Lagged(_)) => {}
                    Err(RecvError::Closed) => break,
                },
                _ = tick.tick() => {
                    self.reload().await;
                    self.blocked = self.engine.context().clients().blocked().into_iter().collect();
                    self.compare(None, false);
                }
            }
        }
    }

    /// Reads the head revision when it moved.
    async fn reload(&mut self) {
        match self.repo.head_revision().await {
            Ok(Some(head)) if self.head.as_ref().is_none_or(|(id, _)| *id != head.meta.id) => {
                self.head = Some((head.meta.id, Arc::new(head.config)));
            }
            Ok(_) => {}
            Err(err) => warn!(%err, "client bans: cannot read the site"),
        }
    }

    fn on_job(&mut self, job: &Job) {
        if job.state == JobState::Running {
            let nodes = job.node.into_iter().chain(job.nodes.iter().map(|n| n.node));
            self.jobs.insert(job.id, nodes.collect());
        } else if let Some(nodes) = self.jobs.remove(&job.id) {
            // What the job left (an apply's `wifi reload` restarts BSSes) gets every ban.
            for node in nodes {
                self.compare(Some(node), true);
            }
        }
    }

    /// The bans wanted now; kicks that ended are forgotten.
    fn wanted(&mut self, now: Timestamp) -> Wanted {
        self.kicked.retain(|_, until| *until > now);
        let mut wanted: Wanted = self.kicked.iter().map(|(m, u)| (*m, Some(*u))).collect();
        wanted.extend(self.blocked.iter().map(|mac| (*mac, None)));
        wanted
    }

    /// The managed nodes online: those whose saved config manages `wireless`.
    fn managed(&self) -> Vec<NodeId> {
        let Some((_, config)) = &self.head else {
            return Vec::new();
        };
        config
            .nodes
            .iter()
            .filter(|n| n.managed.contains(&ManagedPackage::Wireless))
            .map(|n| n.node)
            .filter(|&node| {
                self.engine
                    .node(node)
                    .is_some_and(|view| view.status.state != NodeState::Offline)
            })
            .collect()
    }

    fn busy(&self, node: NodeId) -> bool {
        self.jobs.values().any(|nodes| nodes.contains(&node))
    }

    /// Takes in changed clients: blocks and unblocks, and wanted clients seen connected.
    fn on_clients(&mut self, changes: &Changes) {
        let mut moved = false;
        for view in &changes.clients {
            moved |= if view.blocked.is_some() {
                self.blocked.insert(view.mac)
            } else {
                self.blocked.remove(&view.mac)
            };
        }
        for mac in &changes.removed {
            moved |= self.blocked.remove(mac);
        }
        if moved {
            self.compare(None, false);
        }
        let now = self.engine.context().clock.now();
        let wanted = self.wanted(now);
        let seen: BTreeSet<NodeId> = changes
            .clients
            .iter()
            .filter(|view| view.online && wanted.contains_key(&view.mac))
            .filter_map(|view| view.connection.as_ref())
            .filter(|c| c.medium == ClientMedium::Wifi)
            .filter_map(|c| c.node)
            .collect();
        for node in seen {
            self.compare(Some(node), true);
        }
    }

    /// Sends the nodes (or `only`) what they lack; `force` sends all again unless they were sent
    /// it within `settle`.
    fn compare(&mut self, only: Option<NodeId>, force: bool) {
        let now = self.engine.context().clock.now();
        let wanted = self.wanted(now);
        let terms = self.terms();
        for node in self.managed() {
            if only.is_some_and(|only| only != node) || self.busy(node) {
                continue;
            }
            let bans = self.nodes.entry(node).or_default();
            let since = bans.sent.map(|sent| now.saturating_duration_since(sent));
            let wait = if bans.failing {
                self.config.retry
            } else if force {
                self.config.settle
            } else {
                Duration::ZERO
            };
            if bans.running > 0 || since.is_some_and(|since| since < wait) {
                continue;
            }
            if let Some(entries) = plan::request(&wanted, &bans.held, now, terms, force, None) {
                self.send(node, entries, now, None);
            }
        }
    }

    fn terms(&self) -> Terms {
        Terms {
            chunk: self.config.chunk,
            renew_before: self.config.renew_before,
        }
    }

    /// Kicks `mac` off every managed node (even one a job holds), and announces it.
    fn kick(
        &mut self,
        mac: MacAddr,
        ban: Duration,
        by: Option<String>,
        reply: oneshot::Sender<Kicked>,
    ) {
        let now = self.engine.context().clock.now();
        if ban.is_zero() {
            self.kicked.remove(&mac);
        } else {
            let until = now.saturating_add(ban);
            let was = self.kicked.get(&mac).copied().unwrap_or(until);
            self.kicked.insert(mac, until.max(was));
        }
        let wanted = self.wanted(now);
        let terms = self.terms();
        let nodes = self.managed();
        let ban_secs = u32::try_from(ban.as_secs()).unwrap_or(u32::MAX);
        let event = NewEvent::new(now, EventKind::ClientKicked { ban_secs, by }).client(mac);
        self.engine.note(event);
        let mut asked = JoinSet::new();
        for node in nodes {
            let bans = self.nodes.entry(node).or_default();
            let Some(entries) = plan::request(&wanted, &bans.held, now, terms, true, Some(mac))
            else {
                continue;
            };
            if let Some(request) = self.send(node, entries, now, Some(())) {
                asked.spawn(request);
            }
        }
        tokio::spawn(async move {
            let mut kicked = Kicked {
                nodes: u32::try_from(asked.len()).unwrap_or(u32::MAX),
                ..Kicked::default()
            };
            while let Some(Ok((node, result))) = asked.join_next().await {
                match result {
                    Ok(banned) => kicked.from.extend(
                        banned
                            .kicked
                            .into_iter()
                            .filter(|k| k.addr.parse::<MacAddr>() == Ok(mac))
                            .map(|k| KickedFrom {
                                node,
                                ifname: k.ifname,
                            }),
                    ),
                    Err(error) => kicked.failed.push(KickFailure { node, error }),
                }
            }
            let _ = reply.send(kicked);
        });
    }

    /// Sends a node `entries`. With `answer`, returns a future that waits for the node and
    /// answers what it did (the task hears of it too); else the request runs on its own.
    fn send(
        &mut self,
        node: NodeId,
        entries: Vec<BanEntry>,
        now: Timestamp,
        answer: Option<()>,
    ) -> Option<impl Future<Output = (NodeId, Result<Banned, String>)> + Send + 'static> {
        let link = self.engine.link(node)?;
        let bans = self.nodes.entry(node).or_default();
        bans.sent = Some(now);
        bans.running += 1;
        debug!(%node, bans = entries.len(), "client bans: sending");
        let (done, timeout) = (self.done.0.clone(), self.config.timeout);
        let request = async move {
            let ban = Ban {
                bans: entries.clone(),
            };
            let result = link.request(&ban, timeout).await.map_err(|err| match err {
                // What the node said, for people; the code is for the controller.
                LinkError::Remote { msg, .. } => msg,
                other => other.to_string(),
            });
            let _ = done
                .send(Done {
                    node,
                    at: now,
                    entries,
                    result: result.clone(),
                })
                .await;
            (node, result)
        };
        if answer.is_some() {
            return Some(request);
        }
        tokio::spawn(request);
        None
    }

    fn on_done(
        &mut self,
        Done {
            node,
            at,
            entries,
            result,
        }: Done,
    ) {
        let now = self.engine.context().clock.now();
        let bans = self.nodes.entry(node).or_default();
        bans.running = bans.running.saturating_sub(1);
        let condition = format!("node:{node}/client-bans");
        let error = match result {
            Ok(banned) if banned.failed.is_empty() => None,
            Ok(banned) => banned
                .failed
                .into_iter()
                .next()
                .map(|f| format!("{}: {}", f.ifname, f.error)),
            Err(error) => Some(error),
        };
        match error {
            None => {
                plan::took(&mut bans.held, &entries, at, now);
                if std::mem::take(&mut bans.failing) {
                    let event = NewEvent::new(now, EventKind::ClientBansOk).node(node);
                    self.engine.note(event.condition(condition, false));
                }
            }
            Some(error) => {
                warn!(%node, %error, "client bans: the node refused");
                if !std::mem::replace(&mut bans.failing, true) {
                    let what = EventKind::ClientBansFailed { error };
                    let event = NewEvent::new(now, what).node(node);
                    self.engine
                        .note(event.condition(condition.clone(), true).fold(condition));
                }
            }
        }
    }
}
