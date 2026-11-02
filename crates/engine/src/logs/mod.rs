//! Recording logs (features/observability.md, Logs; D-059): every watched node's log, followed
//! by its agent or received by syslog ([`source`]), and other hosts' syslog, annotated
//! (`uniwrt_syslog::Patterns`), stored in batches through [`LogRepo`] and announced as
//! [`Event::Logged`] once stored; retention runs beside it.
//!
//! Intake never waits on the store: batches go to a writer task, so the bus is read
//! as fast as it fills.

mod source;

use std::{
    collections::{BTreeMap, HashMap},
    net::IpAddr,
    sync::Arc,
    time::Duration,
};

use tokio::{
    sync::{
        broadcast::{self, error::RecvError},
        mpsc, oneshot,
    },
    task::JoinHandle,
    time::Instant,
};
use tracing::{debug, info, warn};
use uniwrt_core::{NodeId, Timestamp};
use uniwrt_model::log::{LogEntry, LogVia, NewLog};
use uniwrt_node::{NodeLink, proto::ops::Set};
use uniwrt_store::{LogRepo, LogsPrune};
use uniwrt_syslog::{Patterns, Received};

use self::source::{Key, NodeLog};
use crate::{Engine, Event, engine::Context};

#[derive(Debug, Clone, Copy)]
pub struct LogsConfig {
    /// How long entries gather before they are stored together, and the most in one batch.
    pub flush_every: Duration,
    pub max_batch: usize,
    /// Entries are kept this long, and while they take at most `max_bytes` (estimated).
    pub retention: Duration,
    pub max_bytes: u64,
    pub prune_every: Duration,
    /// How long a node's syslog over UDP may be quiet before its agent follows the log again.
    pub syslog_quiet: Duration,
}

impl Default for LogsConfig {
    fn default() -> Self {
        Self {
            flush_every: Duration::from_millis(250),
            max_batch: 512,
            retention: Duration::from_hours(14 * 24),
            max_bytes: 1 << 30,
            prune_every: Duration::from_mins(15),
            syslog_quiet: Duration::from_mins(10),
        }
    }
}

/// Batches waiting for the writer; intake waits when they are all taken.
const QUEUED_BATCHES: usize = 64;
/// The most prunes in one round; a longer catch-up goes on next round.
const PRUNES_PER_ROUND: usize = 20;
/// How often quiet syslog is looked for.
const QUIET_CHECK: Duration = Duration::from_secs(30);

/// Records logs until stopped.
#[derive(Debug)]
pub struct LogRecorder {
    stop: oneshot::Sender<()>,
    tasks: [JoinHandle<()>; 3],
}

impl LogRecorder {
    /// Starts following every node the engine watches (and those it watches later), resuming
    /// after the entries already stored, and takes in `syslog` when given.
    pub async fn start<L: NodeLink, S: LogRepo + Clone + 'static>(
        engine: &Engine<L>,
        repo: S,
        syslog: Option<mpsc::Receiver<Received>>,
        config: LogsConfig,
    ) -> Self {
        let bus = engine.subscribe();
        let mut intake = Intake {
            engine: engine.clone(),
            config,
            nodes: BTreeMap::new(),
            peers: Peers::default(),
            pending: Vec::new(),
        };
        for (node, _) in engine.all() {
            intake.watch(node, &repo).await;
        }
        let (batches, queued) = mpsc::channel(QUEUED_BATCHES);
        let (stop, stopped) = oneshot::channel();
        let (stop_upkeep, upkeep_stopped) = oneshot::channel();
        let ctx = Arc::clone(engine.context());
        let writer = tokio::spawn(write(Arc::clone(&ctx), repo.clone(), queued));
        let intake =
            tokio::spawn(intake.run(bus, syslog, repo.clone(), batches, stopped, stop_upkeep));
        let upkeep = tokio::spawn(upkeep(ctx, repo, config, upkeep_stopped));
        Self {
            stop,
            tasks: [intake, writer, upkeep],
        }
    }

    /// Stores what was taken in, and stops.
    pub async fn stop(self) {
        let _ = self.stop.send(());
        for task in self.tasks {
            let _ = task.await;
        }
    }
}

struct Intake<L: NodeLink> {
    engine: Engine<L>,
    config: LogsConfig,
    nodes: BTreeMap<NodeId, NodeLog>,
    peers: Peers,
    pending: Vec<NewLog>,
}

impl<L: NodeLink> Intake<L> {
    async fn run<S: LogRepo>(
        mut self,
        mut bus: broadcast::Receiver<Event>,
        mut syslog: Option<mpsc::Receiver<Received>>,
        repo: S,
        batches: mpsc::Sender<Vec<NewLog>>,
        mut stopped: oneshot::Receiver<()>,
        stop_upkeep: oneshot::Sender<()>,
    ) {
        let mut flush = tokio::time::interval(self.config.flush_every);
        let mut quiet = tokio::time::interval(QUIET_CHECK.min(self.config.syslog_quiet));
        for timer in [&mut flush, &mut quiet] {
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        }
        loop {
            tokio::select! {
                event = bus.recv() => match event {
                    Ok(event) => self.on_event(event, &repo).await,
                    Err(RecvError::Lagged(missed)) => {
                        warn!(missed, "the log recorder fell behind the bus: entries may be lost");
                    }
                    Err(RecvError::Closed) => break,
                },
                Some(received) = recv(&mut syslog) => self.on_syslog(received),
                _ = flush.tick() => self.flush(&batches).await,
                _ = quiet.tick() => self.hand_back_quiet(),
                _ = &mut stopped => {
                    while let Ok(event) = bus.try_recv() {
                        self.on_event(event, &repo).await;
                    }
                    break;
                }
            }
            if self.pending.len() >= self.config.max_batch {
                self.flush(&batches).await;
            }
        }
        self.flush(&batches).await;
        let _ = stop_upkeep.send(());
    }

    /// Starts on a node the engine watches: the agent follows its log, after what is stored.
    async fn watch<S: LogRepo>(&mut self, node: NodeId, repo: &S) {
        if self.nodes.contains_key(&node) {
            return;
        }
        let last = match repo.last_log(node).await {
            Ok(last) => last.map(|record| Key::of(&record.entry)),
            Err(err) => {
                warn!(node_id = %node, %err, "cannot read the node's last stored log entry");
                None
            }
        };
        let mut log = NodeLog::new(last);
        log.follow(self.engine.follow_logs(node));
        self.nodes.insert(node, log);
    }

    async fn on_event<S: LogRepo>(&mut self, event: Event, repo: &S) {
        match event {
            Event::Log {
                node,
                entries,
                dropped,
            } => {
                if dropped > 0 {
                    debug!(node_id = %node, dropped, "log entries lost on the node's side");
                }
                self.watch(node, repo).await;
                let Some(log) = self.nodes.get_mut(&node) else {
                    return;
                };
                let (now, received) = (Instant::now(), self.engine.context().clock.now());
                for entry in log.agent_batch(&entries, now) {
                    self.pending
                        .push(annotated(Some(node), None, LogVia::Agent, received, entry));
                }
            }
            Event::Status { node, .. } => self.watch(node, repo).await,
            Event::Facts { node, changed, .. } => {
                if changed
                    .iter()
                    .any(|set| matches!(set, Set::Interfaces | Set::Board))
                {
                    self.peers.stale = true;
                }
                self.watch(node, repo).await;
            }
            Event::Removed { node } => {
                self.nodes.remove(&node);
                self.peers.stale = true;
            }
            Event::Node { .. }
            | Event::Roam { .. }
            | Event::Topology { .. }
            | Event::Clients { .. }
            | Event::Logged { .. }
            | Event::Recorded { .. } => {}
        }
    }

    fn on_syslog(&mut self, received: Received) {
        match received {
            Received::Message {
                peer,
                at,
                tcp,
                message,
            } => {
                let now = Instant::now();
                let node = self
                    .peers
                    .node(&self.engine, peer, message.hostname.as_deref())
                    .filter(|node| self.nodes.contains_key(node));
                let Some(node) = node else {
                    let host = message.hostname.unwrap_or_else(|| peer.to_string());
                    self.pending
                        .push(annotated_at(None, Some(host), at, message.entry));
                    return;
                };
                let Some(log) = self.nodes.get_mut(&node) else {
                    return;
                };
                if log.syslog_arrived(now, tcp) {
                    info!(node_id = %node, %peer, "the node's log arrives by syslog: the agent stops following it");
                }
                if log.keep(&message.entry, LogVia::Syslog, now) {
                    self.pending
                        .push(annotated_at(Some(node), None, at, message.entry));
                }
            }
            Received::Closed { peer } => {
                let Some(node) = self.peers.node(&self.engine, peer, None) else {
                    return;
                };
                if let Some(log) = self.nodes.get_mut(&node)
                    && log.syslog_tcp()
                {
                    info!(node_id = %node, "the node's syslog connection closed: the agent follows its log again");
                    log.follow(self.engine.follow_logs(node));
                }
            }
        }
    }

    /// Nodes whose syslog over UDP went quiet: their agents follow the log again.
    fn hand_back_quiet(&mut self) {
        let now = Instant::now();
        for (&node, log) in &mut self.nodes {
            if log.syslog_quiet(now, self.config.syslog_quiet) {
                info!(node_id = %node, "the node's syslog went quiet: the agent follows its log again");
                log.follow(self.engine.follow_logs(node));
            }
        }
    }

    async fn flush(&mut self, batches: &mpsc::Sender<Vec<NewLog>>) {
        if !self.pending.is_empty() {
            let batch = std::mem::take(&mut self.pending);
            let _ = batches.send(batch).await;
        }
    }
}

async fn recv(syslog: &mut Option<mpsc::Receiver<Received>>) -> Option<Received> {
    match syslog {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}

fn annotated(
    node: Option<NodeId>,
    host: Option<String>,
    via: LogVia,
    received: Timestamp,
    entry: LogEntry,
) -> NewLog {
    NewLog {
        received,
        node,
        host,
        via,
        pattern: Patterns::embedded()
            .annotate(&entry)
            .map(|pattern| pattern.id.clone()),
        entry,
    }
}

/// A syslog entry, which arrived at `at`.
fn annotated_at(
    node: Option<NodeId>,
    host: Option<String>,
    at: Timestamp,
    entry: LogEntry,
) -> NewLog {
    annotated(node, host, LogVia::Syslog, at, entry)
}

/// Which node a syslog sender is: by its address among the nodes' interface addresses, else
/// by the host name its messages give. Loopback addresses (every node has 127.0.0.1) and
/// addresses or names that several nodes share (`OpenWrt`, an unconfigured default) name none.
#[derive(Default)]
struct Peers {
    /// `None` where several nodes claim it.
    addresses: HashMap<IpAddr, Option<NodeId>>,
    hostnames: HashMap<String, Option<NodeId>>,
    stale: bool,
    built: bool,
}

impl Peers {
    fn node<L: NodeLink>(
        &mut self,
        engine: &Engine<L>,
        peer: IpAddr,
        hostname: Option<&str>,
    ) -> Option<NodeId> {
        if self.stale || !self.built {
            self.rebuild(engine);
        }
        self.addresses.get(&peer).copied().flatten().or_else(|| {
            hostname.and_then(|name| {
                self.hostnames
                    .get(&name.to_ascii_lowercase())
                    .copied()
                    .flatten()
            })
        })
    }

    fn rebuild<L: NodeLink>(&mut self, engine: &Engine<L>) {
        self.addresses.clear();
        self.hostnames.clear();
        for (node, view) in engine.all() {
            let Some(facts) = view.facts else { continue };
            for interface in &facts.interfaces {
                let v4 = interface.ipv4.iter().map(|net| IpAddr::V4(net.addr()));
                let v6 = interface.ipv6.iter().map(|net| IpAddr::V6(net.addr()));
                for address in v4.chain(v6) {
                    if !address.is_loopback() && !address.is_unspecified() {
                        claim(&mut self.addresses, address, node);
                    }
                }
            }
            if !facts.board.hostname.is_empty() {
                claim(
                    &mut self.hostnames,
                    facts.board.hostname.to_ascii_lowercase(),
                    node,
                );
            }
        }
        self.stale = false;
        self.built = true;
    }
}

fn claim<K: Eq + std::hash::Hash>(map: &mut HashMap<K, Option<NodeId>>, key: K, node: NodeId) {
    map.entry(key)
        .and_modify(|owner| {
            if *owner != Some(node) {
                *owner = None;
            }
        })
        .or_insert(Some(node));
}

/// Stores batches in order and announces what was stored.
async fn write<S: LogRepo>(ctx: Arc<Context>, repo: S, mut queued: mpsc::Receiver<Vec<NewLog>>) {
    while let Some(batch) = queued.recv().await {
        match repo.record_logs(batch).await {
            Ok(records) => ctx.publish(Event::Logged {
                records: records.into(),
            }),
            Err(err) => warn!(%err, "cannot store log entries"),
        }
    }
}

async fn upkeep<S: LogRepo>(
    ctx: Arc<Context>,
    repo: S,
    config: LogsConfig,
    mut stopped: oneshot::Receiver<()>,
) {
    let mut prune = tokio::time::interval(config.prune_every);
    prune.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = prune.tick() => {
                let before = ctx.clock.now().saturating_sub(config.retention);
                let limits = LogsPrune { before, max_bytes: config.max_bytes };
                for _ in 0..PRUNES_PER_ROUND {
                    match repo.prune_logs(limits).await {
                        Ok(0) => break,
                        Ok(_) => {}
                        Err(err) => {
                            warn!(%err, "cannot prune the logs");
                            break;
                        }
                    }
                }
            }
            _ = &mut stopped => return,
        }
    }
}

#[cfg(test)]
mod tests;
