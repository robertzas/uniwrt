//! The blocklists' task: the gateway and the revision it holds, when to refresh, compile and
//! push, and the state it publishes.

use std::{collections::BTreeMap, sync::Arc};

use bytes::Bytes;
use tokio::sync::{
    broadcast::{self, error::RecvError},
    mpsc, oneshot, watch,
};
use tracing::{info, warn};
use uniwrt_core::{JobId, NodeId, RevisionId, Timestamp};
use uniwrt_dns::{Blocklists, Compiled, Wanted, wanted};
use uniwrt_model::{
    blocklists::{BlocklistState, CompiledStatus, GatewaySync, SyncState, capacity},
    facts::NodeFacts,
    job::{Job, JobState},
    node::NodeRole,
    site::{SiteConfig, blocking::Blocking, node::ManagedPackage, tz},
    status::NodeState,
};
use uniwrt_node::NodeLink;
use uniwrt_store::{ApplyRepo, RevisionRepo};

use super::{
    BlocklistConfig, BlocklistError,
    push::{self, Action},
};
use crate::{Engine, Event, wlans::next_job};

pub(super) enum Command {
    Refresh {
        reply: oneshot::Sender<Result<(), BlocklistError>>,
    },
}

enum Done {
    Refreshed {
        changed: bool,
    },
    Compiled {
        key: Key,
        result: Result<Compiled, String>,
    },
    /// A push of the file named (`None`: the removal).
    Pushed {
        sent: Option<String>,
        result: Result<(), String>,
    },
}

/// What a compile was made of: the applied settings, the gateway's capacity and the copies.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Key {
    wanted: Vec<Wanted>,
    allow: Vec<String>,
    deny: Vec<String>,
    capacity: u32,
    /// Counts the refreshes that changed a copy.
    generation: u64,
}

/// The latest compile; its text until it is pushed.
struct Kept {
    key: Key,
    status: CompiledStatus,
    text: Option<Bytes>,
}

pub(super) struct Task<L, R> {
    engine: Engine<L>,
    repo: R,
    lists: Blocklists,
    config: BlocklistConfig,
    state: watch::Sender<Arc<BlocklistState>>,
    head: Option<(RevisionId, Arc<SiteConfig>)>,
    gateway: Option<NodeId>,
    /// The revision the gateway holds.
    applied: Option<(RevisionId, Arc<SiteConfig>)>,
    /// The nodes each running job holds.
    jobs: BTreeMap<JobId, Vec<NodeId>>,
    generation: u64,
    refreshing: bool,
    /// When a refresh last started.
    refreshed: Option<Timestamp>,
    compiling: bool,
    compiled: Option<Kept>,
    /// The compile that failed, when and why.
    compile_failed: Option<(Key, Timestamp, String)>,
    pushing: bool,
    /// The last push: the file sent (`None`: the removal) and when.
    pushed: Option<(Option<String>, Timestamp)>,
    /// When a push last went through.
    pushed_ok: Option<Timestamp>,
    push_error: Option<String>,
    done: (mpsc::Sender<Done>, mpsc::Receiver<Done>),
}

/// The state before the first look.
pub(super) fn empty() -> BlocklistState {
    BlocklistState {
        gateway: None,
        enabled: false,
        lists: Vec::new(),
        compiled: None,
        sync: GatewaySync {
            state: SyncState::NoGateway,
            sha256: None,
            domains: None,
            pushed_at: None,
            error: None,
        },
        refreshing: false,
        refreshed_at: None,
    }
}

/// Whether lists least recently asked for at `oldest` (`None`: one never was) are refreshed
/// `now`, `in_window` saying whether the nightly window is open.
pub(super) fn due(
    oldest: Option<Timestamp>,
    now: Timestamp,
    in_window: bool,
    config: &BlocklistConfig,
) -> bool {
    let Some(oldest) = oldest else { return true };
    let age = now.saturating_duration_since(oldest);
    age >= config.overdue || (age >= config.stale && in_window)
}

impl<L, R> Task<L, R>
where
    L: NodeLink,
    R: RevisionRepo + ApplyRepo + Clone + 'static,
{
    pub(super) fn new(
        engine: Engine<L>,
        repo: R,
        lists: Blocklists,
        config: BlocklistConfig,
        state: watch::Sender<Arc<BlocklistState>>,
    ) -> Self {
        Self {
            engine,
            repo,
            lists,
            config,
            state,
            head: None,
            gateway: None,
            applied: None,
            jobs: BTreeMap::new(),
            generation: 0,
            refreshing: false,
            refreshed: None,
            compiling: false,
            compiled: None,
            compile_failed: None,
            pushing: false,
            pushed: None,
            pushed_ok: None,
            push_error: None,
            done: mpsc::channel(8),
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
                Some(Command::Refresh { reply }) = commands.recv() => {
                    self.reload().await;
                    let wanted = self.refresh_wanted();
                    let answer = if wanted.is_empty() {
                        Err(BlocklistError::Off)
                    } else {
                        if !self.refreshing {
                            self.start_refresh(wanted);
                        }
                        Ok(())
                    };
                    // The answer's reader sees the refresh running.
                    self.publish();
                    let _ = reply.send(answer);
                }
                Some(job) = next_job(&mut jobs) => self.on_job(&job),
                Some(done) = self.done.1.recv() => self.on_done(done),
                event = bus.recv() => match event {
                    Ok(Event::Status { node, status }) if Some(node) == self.gateway => {
                        if status.state == NodeState::Offline {
                            // It may come back rebooted, its `/tmp` empty: no wait for it.
                            self.pushed = None;
                        }
                    }
                    Ok(Event::Removed { node }) if Some(node) == self.gateway => {
                        self.gateway = None;
                        self.applied = None;
                    }
                    Ok(_) | Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => break,
                },
                _ = tick.tick() => self.reload().await,
            }
            self.step();
            self.publish();
        }
    }

    /// Reads the head revision, the gateway and the revision it holds, where they moved.
    async fn reload(&mut self) {
        match self.repo.head_revision().await {
            Ok(Some(head)) if self.head.as_ref().is_none_or(|(id, _)| *id != head.meta.id) => {
                self.head = Some((head.meta.id, Arc::new(head.config)));
            }
            Ok(_) => {}
            Err(err) => warn!(%err, "blocklists: cannot read the site"),
        }
        let topology = &self.engine.context().topology;
        self.gateway = self
            .engine
            .all()
            .into_iter()
            .map(|(node, _)| node)
            .find(|&node| topology.role(node) == Some(NodeRole::Gateway));
        let Some(gateway) = self.gateway else {
            self.applied = None;
            return;
        };
        let id = match self.repo.applied_revisions().await {
            Ok(all) => all
                .into_iter()
                .find(|a| a.node == gateway)
                .map(|a| a.revision),
            Err(err) => {
                warn!(%err, "blocklists: cannot read the applied revisions");
                return;
            }
        };
        match id {
            None => self.applied = None,
            Some(id) if self.applied.as_ref().is_some_and(|(at, _)| *at == id) => {}
            Some(id) => match self.head.as_ref().filter(|(head, _)| *head == id) {
                Some((_, config)) => self.applied = Some((id, Arc::clone(config))),
                None => match self.repo.revision(id).await {
                    Ok(revision) => {
                        self.applied = revision.map(|r| (id, Arc::new(r.config)));
                    }
                    Err(err) => warn!(%err, "blocklists: cannot read the applied revision"),
                },
            },
        }
    }

    fn on_job(&mut self, job: &Job) {
        if job.state == JobState::Running {
            let nodes = job.node.into_iter().chain(job.nodes.iter().map(|n| n.node));
            self.jobs.insert(job.id, nodes.collect());
        } else {
            self.jobs.remove(&job.id);
        }
    }

    fn busy(&self, node: NodeId) -> bool {
        self.jobs.values().any(|nodes| nodes.contains(&node))
    }

    /// The gateway's blocking settings as it holds them; `None` when the controller leaves its
    /// files alone (no gateway, nothing applied, or its DHCP and DNS unmanaged).
    fn target(&self) -> Option<&Blocking> {
        let gateway = self.gateway?;
        let (_, config) = self.applied.as_ref()?;
        let managed = config
            .nodes
            .iter()
            .any(|n| n.node == gateway && n.managed.contains(&ManagedPackage::Dhcp));
        managed.then_some(&config.services.blocking)
    }

    /// The lists of the applied and head revisions where blocking is on, each URL once.
    fn refresh_wanted(&self) -> Vec<Wanted> {
        let applied = self.target().filter(|b| b.enabled);
        let head = self.head.as_ref().map(|(_, c)| &c.services.blocking);
        let mut all: Vec<Wanted> = Vec::new();
        for blocking in applied.into_iter().chain(head.filter(|b| b.enabled)) {
            for list in wanted(blocking) {
                if !all.iter().any(|w| w.url == list.url) {
                    all.push(list);
                }
            }
        }
        all
    }

    fn in_window(&self, now: Timestamp) -> bool {
        let offset_s = self
            .head
            .as_ref()
            .and_then(|(_, config)| tz::posix(&config.site.timezone))
            .and_then(|posix| tz::local(posix, now))
            .map_or(0, |local| local.offset_s);
        let (at, len) = (self.config.window_at_min, self.config.window_len_min);
        tz::daily_window(now, offset_s, at, len).is_some()
    }

    /// What the gateway's compile is made of now; `None` until its memory is known and every
    /// list was asked for once.
    fn key(&self, blocking: &Blocking, facts: Option<&NodeFacts>) -> Option<Key> {
        let memory = facts?.system.memory.total_bytes;
        let wanted = wanted(blocking);
        if memory == 0 || (!wanted.is_empty() && self.lists.attempted(&wanted).is_none()) {
            return None;
        }
        Some(Key {
            wanted,
            allow: blocking.allow.clone(),
            deny: blocking.deny.clone(),
            capacity: capacity(memory),
            generation: self.generation,
        })
    }

    /// Refreshes, compiles and pushes what is due.
    fn step(&mut self) {
        let now = self.engine.context().clock.now();
        let wanted = self.refresh_wanted();
        let quiet = self
            .refreshed
            .is_none_or(|at| now.saturating_duration_since(at) >= self.config.repush);
        if !wanted.is_empty()
            && !self.refreshing
            && quiet
            && due(
                self.lists.attempted(&wanted),
                now,
                self.in_window(now),
                &self.config,
            )
        {
            self.start_refresh(wanted);
        }
        let (Some(blocking), Some(gateway)) = (self.target().cloned(), self.gateway) else {
            return;
        };
        let Some(view) = self.engine.node(gateway) else {
            return;
        };
        let facts = view.facts.as_deref();
        let held = facts.map_or_else(Default::default, |f| f.services.blocklist.clone());
        let key = if blocking.enabled {
            match self.key(&blocking, facts) {
                Some(key) => Some(key),
                None => return,
            }
        } else {
            None
        };
        let current = self
            .compiled
            .as_ref()
            .filter(|kept| Some(&kept.key) == key.as_ref());
        let Some(key) = key else {
            // Off: whatever the gateway holds goes.
            if push::action(&held, false, None).is_some() && self.may_push(gateway, None, now) {
                self.start_push(gateway, Action::Remove, None, Bytes::new());
            }
            return;
        };
        let Some(kept) = current else {
            self.compile(key, &blocking, now);
            return;
        };
        let sha256 = kept.status.sha256.clone();
        let Some(action) = push::action(&held, true, Some(&sha256)) else {
            return;
        };
        let sent = Some(sha256);
        if !self.may_push(gateway, sent.as_deref(), now) {
            return;
        }
        match kept.text.clone() {
            Some(text) => self.start_push(gateway, action, sent, text),
            // Pushed before (the gateway rebooted since): compiled again.
            None => self.compile(key, &blocking, now),
        }
    }

    /// Whether the gateway may be sent `sent` now: online, no job holding it, no push running,
    /// and the same not sent within `repush`.
    fn may_push(&self, gateway: NodeId, sent: Option<&str>, now: Timestamp) -> bool {
        let online = self
            .engine
            .node(gateway)
            .is_some_and(|view| view.status.state != NodeState::Offline);
        let recent = self.pushed.as_ref().is_some_and(|(what, at)| {
            what.as_deref() == sent && now.saturating_duration_since(*at) < self.config.repush
        });
        online && !self.pushing && !self.busy(gateway) && !recent
    }

    fn start_refresh(&mut self, wanted: Vec<Wanted>) {
        let now = self.engine.context().clock.now();
        self.refreshing = true;
        self.refreshed = Some(now);
        let (lists, done) = (self.lists.clone(), self.done.0.clone());
        tokio::spawn(async move {
            let changed = lists.refresh(&wanted, now).await;
            let _ = done.send(Done::Refreshed { changed }).await;
        });
    }

    fn compile(&mut self, key: Key, blocking: &Blocking, now: Timestamp) {
        let failed_lately = self.compile_failed.as_ref().is_some_and(|(failed, at, _)| {
            *failed == key && now.saturating_duration_since(*at) < self.config.repush
        });
        if self.compiling || self.refreshing || failed_lately {
            return;
        }
        self.compiling = true;
        let (lists, done, blocking) = (self.lists.clone(), self.done.0.clone(), blocking.clone());
        tokio::spawn(async move {
            let result = lists
                .compile(&key.wanted, &blocking, key.capacity)
                .await
                .map_err(|err| err.to_string());
            let _ = done.send(Done::Compiled { key, result }).await;
        });
    }

    fn start_push(&mut self, gateway: NodeId, action: Action, sent: Option<String>, text: Bytes) {
        let Some(link) = self.engine.link(gateway) else {
            return;
        };
        let now = self.engine.context().clock.now();
        self.pushing = true;
        self.pushed = Some((sent.clone(), now));
        let (done, timeout) = (self.done.0.clone(), self.config.timeout);
        tokio::spawn(async move {
            let result = match action {
                Action::Push { restart } => push::push(&*link, text, restart, timeout).await,
                Action::Remove => push::remove(&*link, timeout).await,
            };
            let _ = done.send(Done::Pushed { sent, result }).await;
        });
    }

    fn on_done(&mut self, done: Done) {
        let now = self.engine.context().clock.now();
        match done {
            Done::Refreshed { changed } => {
                self.refreshing = false;
                if changed {
                    self.generation += 1;
                }
            }
            Done::Compiled { key, result } => {
                self.compiling = false;
                match result {
                    Ok(compiled) => {
                        self.compile_failed = None;
                        // The same file keeps the time it was first compiled.
                        let at = self
                            .compiled
                            .as_ref()
                            .filter(|kept| kept.status.sha256 == compiled.sha256)
                            .map_or(now, |kept| kept.status.at);
                        let status = CompiledStatus {
                            sha256: compiled.sha256,
                            domains: compiled.domains,
                            cut: compiled.cut,
                            capacity: key.capacity,
                            at,
                        };
                        let text = Some(Bytes::from(compiled.text));
                        self.compiled = Some(Kept { key, status, text });
                    }
                    Err(err) => {
                        warn!(%err, "blocklists: cannot compile");
                        self.compile_failed = Some((key, now, err));
                    }
                }
            }
            Done::Pushed { sent, result } => {
                self.pushing = false;
                match result {
                    Ok(()) => {
                        info!(file = ?sent, "blocklists: the gateway has its blocklist");
                        self.push_error = None;
                        self.pushed_ok = Some(now);
                        if let Some(kept) = &mut self.compiled
                            && Some(&kept.status.sha256) == sent.as_ref()
                        {
                            kept.text = None;
                        }
                    }
                    Err(err) => {
                        warn!(%err, "blocklists: cannot push to the gateway");
                        self.push_error = Some(err);
                    }
                }
            }
        }
    }

    fn publish(&self) {
        let now = self.engine.context().clock.now();
        let wanted = self.refresh_wanted();
        let enabled = self.target().is_some_and(|b| b.enabled);
        let state = BlocklistState {
            gateway: self.gateway,
            enabled,
            lists: self.lists.status(&wanted),
            compiled: self
                .compiled
                .as_ref()
                .filter(|_| enabled)
                .map(|kept| kept.status.clone()),
            sync: self.sync(enabled, now),
            refreshing: self.refreshing,
            refreshed_at: self.lists.attempted(&wanted),
        };
        self.state.send_if_modified(|current| {
            let moved = **current != state;
            if moved {
                *current = Arc::new(state);
            }
            moved
        });
    }

    fn sync(&self, enabled: bool, now: Timestamp) -> GatewaySync {
        let view = self.gateway.and_then(|node| self.engine.node(node));
        let held = view
            .as_ref()
            .and_then(|v| v.facts.as_ref())
            .map_or_else(Default::default, |f| f.services.blocklist.clone());
        let compiled = self.compiled.as_ref().map(|kept| &kept.status.sha256);
        let offline = view.is_none_or(|v| v.status.state == NodeState::Offline);
        let sent_lately = self.pushed.as_ref().is_some_and(|(sent, at)| {
            sent.as_ref() == compiled && now.saturating_duration_since(*at) < self.config.repush
        });
        let state = if self.gateway.is_none() {
            SyncState::NoGateway
        } else if self.target().is_none() {
            SyncState::Unmanaged
        } else if !enabled {
            SyncState::Off
        } else if offline {
            SyncState::Offline
        } else if compiled.is_some() && held.sha256.as_ref() == compiled && held.conf {
            SyncState::Synced
        } else if self.pushing || (sent_lately && self.push_error.is_none()) {
            SyncState::Pushing
        } else if self.gateway.is_some_and(|g| self.busy(g)) {
            SyncState::Busy
        } else if self.push_error.is_some() {
            SyncState::Failed
        } else {
            SyncState::Waiting
        };
        let error = self
            .push_error
            .clone()
            .or_else(|| self.compile_failed.as_ref().map(|(_, _, err)| err.clone()));
        GatewaySync {
            state,
            sha256: held.sha256,
            domains: held.domains,
            pushed_at: self.pushed_ok,
            error: error.filter(|_| enabled),
        }
    }
}
