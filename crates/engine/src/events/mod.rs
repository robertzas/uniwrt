//! Recording events (features/observability.md, Events; `uniwrt_events`): a deriver fed by the
//! bus (statuses, facts, topology builds, clients, roams, stored log entries) and by the job
//! runner, ticked for conditions that must last, its events folded and stored in batches
//! through [`EventRepo`] and announced as [`Event::Recorded`] once stored; retention runs
//! beside it.
//!
//! At start the deriver learns the conditions still raised in the store, the clients known and
//! each node's latest facts and status, so a restart neither repeats nor misses what changed.

use std::{collections::HashMap, sync::Arc, time::Duration};

use tokio::{
    sync::{
        broadcast::{self, error::RecvError},
        mpsc, oneshot,
    },
    task::JoinHandle,
};
use tracing::warn;
use uniwrt_core::Timestamp;
use uniwrt_events::{DeriveConfig, Deriver, EventWrite, Folder, NewEvent, Seed};
use uniwrt_model::{
    events::{EventCategory, EventKind, EventRecord},
    job::Job,
    log::Severity,
};
use uniwrt_node::{NodeLink, proto::ops::Set};
use uniwrt_store::{EventRepo, EventsPrune};
use uniwrt_syslog::Patterns;

use crate::{Engine, Event, engine::Context};

#[derive(Debug, Clone)]
pub struct EventsConfig {
    pub derive: DeriveConfig,
    /// How often conditions that must last are looked at.
    pub tick_every: Duration,
    /// How long events gather before they are stored together.
    pub flush_every: Duration,
    /// Repeats within this long of the last one fold into it.
    pub fold_window: Duration,
    /// Events are kept this long after their last repeat, and at most `max_events` of them.
    pub retention: Duration,
    pub max_events: u64,
    pub prune_every: Duration,
    /// The controller's version, recorded as it starts; `None` records no start.
    pub started: Option<String>,
}

impl Default for EventsConfig {
    fn default() -> Self {
        Self {
            derive: DeriveConfig::default(),
            tick_every: Duration::from_secs(5),
            flush_every: Duration::from_millis(250),
            fold_window: Duration::from_mins(5),
            retention: Duration::from_hours(60 * 24),
            max_events: 200_000,
            prune_every: Duration::from_mins(15),
            started: None,
        }
    }
}

/// Batches waiting for the writer; intake waits when they are all taken.
const QUEUED_BATCHES: usize = 16;
/// The most prunes in one round; a longer catch-up goes on next round.
const PRUNES_PER_ROUND: usize = 20;

/// Records events until stopped.
#[derive(Debug)]
pub struct EventRecorder {
    stop: oneshot::Sender<()>,
    tasks: [JoinHandle<()>; 3],
}

impl EventRecorder {
    /// Starts deriving from what the engine announces, and from `jobs` when given.
    pub async fn start<L: NodeLink, S: EventRepo + Clone + 'static>(
        engine: &Engine<L>,
        repo: S,
        jobs: Option<broadcast::Receiver<Arc<Job>>>,
        config: EventsConfig,
    ) -> Self {
        let bus = engine.subscribe();
        let ctx = Arc::clone(engine.context());
        let now = ctx.clock.now();
        let raised = repo.raised_conditions().await.unwrap_or_else(|err| {
            warn!(%err, "cannot read the conditions still raised: they start cleared");
            Vec::new()
        });
        let nodes = engine.all();
        let seed = Seed {
            started: now,
            raised,
            clients: engine.clients(),
            facts: nodes
                .iter()
                .filter_map(|(node, view)| {
                    Some((
                        *node,
                        ctx.topology.role(*node)?,
                        Arc::clone(view.facts.as_ref()?),
                    ))
                })
                .collect(),
        };
        let mut intake = Intake {
            ctx: Arc::clone(&ctx),
            deriver: Deriver::new(config.derive, seed),
            patterns: pattern_events(),
            pending: Vec::new(),
        };
        if let Some(version) = &config.started {
            intake.pending.push(NewEvent::new(
                now,
                EventKind::ControllerStarted {
                    version: version.clone(),
                },
            ));
        }
        for (node, view) in &nodes {
            let events = intake.deriver.status(*node, &view.status, now);
            intake.pending.extend(events);
        }
        let (batches, queued) = mpsc::channel(QUEUED_BATCHES);
        let (stop, stopped) = oneshot::channel();
        let (stop_upkeep, upkeep_stopped) = oneshot::channel();
        let writer = tokio::spawn(write(
            Arc::clone(&ctx),
            repo.clone(),
            Folder::new(config.fold_window),
            queued,
        ));
        let intake =
            tokio::spawn(intake.run(bus, jobs, config.clone(), batches, stopped, stop_upkeep));
        let upkeep = tokio::spawn(upkeep(ctx, repo, config, upkeep_stopped));
        Self {
            stop,
            tasks: [intake, writer, upkeep],
        }
    }

    /// Stores what was derived, and stops.
    pub async fn stop(self) {
        let _ = self.stop.send(());
        for task in self.tasks {
            let _ = task.await;
        }
    }
}

/// The log patterns that are events: their category and severity, by id.
fn pattern_events() -> HashMap<String, (EventCategory, Severity)> {
    Patterns::embedded()
        .all()
        .iter()
        .filter_map(|pattern| Some((pattern.id.clone(), (pattern.event?, pattern.severity))))
        .collect()
}

struct Intake {
    ctx: Arc<Context>,
    deriver: Deriver,
    patterns: HashMap<String, (EventCategory, Severity)>,
    pending: Vec<NewEvent>,
}

impl Intake {
    async fn run(
        mut self,
        mut bus: broadcast::Receiver<Event>,
        mut jobs: Option<broadcast::Receiver<Arc<Job>>>,
        config: EventsConfig,
        batches: mpsc::Sender<Vec<NewEvent>>,
        mut stopped: oneshot::Receiver<()>,
        stop_upkeep: oneshot::Sender<()>,
    ) {
        let mut flush = tokio::time::interval(config.flush_every);
        let mut tick = tokio::time::interval(config.tick_every);
        for timer in [&mut flush, &mut tick] {
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        }
        loop {
            tokio::select! {
                event = bus.recv() => match event {
                    Ok(event) => self.on_event(&event),
                    Err(RecvError::Lagged(missed)) => {
                        warn!(missed, "the event recorder fell behind the bus: events may be lost");
                    }
                    Err(RecvError::Closed) => break,
                },
                Some(job) = next_job(&mut jobs) => {
                    let now = self.now();
                    self.pending.extend(self.deriver.job(&job, now));
                }
                _ = tick.tick() => {
                    let now = self.now();
                    self.pending.extend(self.deriver.tick(now));
                }
                _ = flush.tick() => self.flush(&batches).await,
                _ = &mut stopped => {
                    while let Ok(event) = bus.try_recv() {
                        self.on_event(&event);
                    }
                    break;
                }
            }
        }
        self.flush(&batches).await;
        let _ = stop_upkeep.send(());
    }

    fn now(&self) -> Timestamp {
        self.ctx.clock.now()
    }

    fn on_event(&mut self, event: &Event) {
        let now = self.now();
        let deriver = &mut self.deriver;
        let events = match event {
            Event::Status { node, status } => deriver.status(*node, status, now),
            Event::Facts {
                node,
                facts,
                changed,
            } => match self.ctx.topology.role(*node) {
                Some(role) => {
                    let fresh = changed.contains(&Set::System);
                    deriver.facts(*node, role, facts, fresh, now)
                }
                None => Vec::new(),
            },
            Event::Topology { topology } => deriver.topology(topology, now),
            Event::Clients { changes } => {
                deriver.clients_removed(&changes.removed);
                deriver.clients(&changes.clients, now)
            }
            Event::Roam { at, roam } => vec![deriver.roam(roam, *at)],
            Event::Logged { records } => {
                let patterns = &self.patterns;
                deriver.logged(records, |id| patterns.get(id).copied())
            }
            Event::Removed { node } => {
                deriver.removed(*node);
                Vec::new()
            }
            Event::Drift { changes, .. } => deriver.drift(changes, now),
            Event::Node { .. }
            | Event::Log { .. }
            | Event::Recorded { .. }
            | Event::Sampled { .. }
            | Event::Alerts { .. }
            | Event::Rf { .. } => Vec::new(),
        };
        self.pending.extend(events);
    }

    async fn flush(&mut self, batches: &mpsc::Sender<Vec<NewEvent>>) {
        if !self.pending.is_empty() {
            let batch = std::mem::take(&mut self.pending);
            let _ = batches.send(batch).await;
        }
    }
}

async fn next_job(jobs: &mut Option<broadcast::Receiver<Arc<Job>>>) -> Option<Arc<Job>> {
    let Some(rx) = jobs else {
        return std::future::pending().await;
    };
    loop {
        match rx.recv().await {
            Ok(job) => return Some(job),
            Err(RecvError::Lagged(missed)) => {
                warn!(
                    missed,
                    "the event recorder fell behind the jobs: events may be lost"
                );
            }
            Err(RecvError::Closed) => {
                *jobs = None;
                return std::future::pending().await;
            }
        }
    }
}

/// Folds, stores batches in order and announces what was stored.
async fn write<S: EventRepo>(
    ctx: Arc<Context>,
    repo: S,
    mut folder: Folder,
    mut queued: mpsc::Receiver<Vec<NewEvent>>,
) {
    while let Some(batch) = queued.recv().await {
        folder.expire(ctx.clock.now());
        let writes = folder.plan(batch);
        let keys: Vec<Option<String>> = writes
            .iter()
            .map(|write| match write {
                EventWrite::Insert(event) => event.fold.clone(),
                EventWrite::Fold { .. } => None,
            })
            .collect();
        match repo.record_events(writes).await {
            Ok(records) => {
                let mut stored: Vec<EventRecord> = Vec::with_capacity(records.len());
                for (key, record) in keys.into_iter().zip(records) {
                    let Some(record) = record else { continue };
                    if let Some(key) = key {
                        folder.opened(key, record.id, record.last_at);
                    }
                    stored.push(record);
                }
                if !stored.is_empty() {
                    ctx.publish(Event::Recorded {
                        events: stored.into(),
                    });
                }
            }
            Err(err) => warn!(%err, "cannot store events"),
        }
    }
}

async fn upkeep<S: EventRepo>(
    ctx: Arc<Context>,
    repo: S,
    config: EventsConfig,
    mut stopped: oneshot::Receiver<()>,
) {
    let mut prune = tokio::time::interval(config.prune_every);
    prune.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = prune.tick() => {
                let before = ctx.clock.now().saturating_sub(config.retention);
                let limits = EventsPrune { before, max_events: config.max_events };
                for _ in 0..PRUNES_PER_ROUND {
                    match repo.prune_events(limits).await {
                        Ok(0) => break,
                        Ok(_) => {}
                        Err(err) => {
                            warn!(%err, "cannot prune the events");
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
