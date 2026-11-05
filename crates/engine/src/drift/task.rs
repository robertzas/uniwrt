//! The drift watcher's task: what it knows of each node (confirmed packages, drifts, mutes, the
//! agent's hashes, the jobs that hold it), its loop, and what it announces. How it looks at a
//! node is in `look.rs`, its reads in `fetch.rs`, what users do in `user.rs`.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use tokio::sync::{
    broadcast::{self, error::RecvError},
    mpsc, oneshot, watch,
};
use tracing::warn;
use uniwrt_core::{JobId, NodeId, Timestamp};
use uniwrt_model::{
    drift::{Confirmed, Drift, DriftChange, DriftMute, DriftRecord, DriftState},
    job::{Job, JobState},
};
use uniwrt_node::{NodeLink, proto::ops::Set};
use uniwrt_store::DriftRepo;

use super::{Command, DriftConfig, DriftError, view::Key};
use crate::{Engine, Event};

mod fetch;
mod look;
mod user;

pub(crate) use fetch::Reading;

pub(super) struct Task<L: NodeLink, R> {
    engine: Engine<L>,
    repo: R,
    config: DriftConfig,
    commands: mpsc::Sender<Command>,
    state: watch::Sender<Arc<DriftState>>,
    confirmed: BTreeMap<Key, Confirmed>,
    records: BTreeMap<Key, DriftRecord>,
    views: BTreeMap<Key, Drift>,
    mutes: BTreeMap<Key, DriftMute>,
    /// The agent's latest hashes, by node.
    hashes: BTreeMap<NodeId, BTreeMap<String, String>>,
    /// The nodes each running job holds.
    jobs: BTreeMap<JobId, Vec<NodeId>>,
    /// Nodes with a read task out.
    reading: BTreeSet<NodeId>,
    changes: Vec<DriftChange>,
    /// Drifts ended, to forget in the store before anything else is stored.
    ended: Vec<Key>,
}

impl<L: NodeLink, R: DriftRepo + Clone + 'static> Task<L, R> {
    pub(super) async fn load(
        engine: Engine<L>,
        repo: R,
        config: DriftConfig,
        commands: mpsc::Sender<Command>,
    ) -> Result<(Self, watch::Receiver<Arc<DriftState>>), DriftError> {
        let key = |node: NodeId, package: &str| (node, package.to_owned());
        let confirmed = repo
            .confirmed(None)
            .await?
            .into_iter()
            .map(|c| (key(c.node, &c.package), c))
            .collect();
        let records = repo
            .drifts()
            .await?
            .into_iter()
            .map(|r| (key(r.node, &r.package), r))
            .collect();
        let mutes = repo
            .drift_mutes()
            .await?
            .into_iter()
            .map(|m| (key(m.node, &m.package), m))
            .collect();
        let hashes = engine
            .all()
            .into_iter()
            .filter_map(|(node, view)| Some((node, view.facts?.config.clone())))
            .collect();
        let (state, watched) = watch::channel(Arc::new(DriftState::default()));
        let mut task = Self {
            engine,
            repo,
            config,
            commands,
            state,
            confirmed,
            records,
            views: BTreeMap::new(),
            mutes,
            hashes,
            jobs: BTreeMap::new(),
            reading: BTreeSet::new(),
            changes: Vec::new(),
            ended: Vec::new(),
        };
        let keys: Vec<Key> = task.records.keys().cloned().collect();
        for key in keys {
            task.refresh_view(&key);
        }
        task.state.send_replace(Arc::new(task.snapshot()));
        Ok((task, watched))
    }

    pub(super) async fn run(
        mut self,
        mut bus: broadcast::Receiver<Event>,
        mut jobs: Option<broadcast::Receiver<Arc<Job>>>,
        mut commands: mpsc::Receiver<Command>,
        mut stopped: oneshot::Receiver<()>,
    ) {
        let nodes: Vec<NodeId> = self.hashes.keys().copied().collect();
        for node in nodes {
            self.look(node);
        }
        self.publish().await;
        let mut tick = tokio::time::interval(self.config.tick_every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        tick.tick().await;
        loop {
            // Jobs and commands first: what a user or a job did comes before the node events
            // that follow it.
            tokio::select! {
                biased;
                _ = &mut stopped => break,
                Some(job) = next_job(&mut jobs) => self.on_job(&job).await,
                Some(command) = commands.recv() => self.on_command(command).await,
                event = bus.recv() => match event {
                    Ok(event) => self.on_event(&event),
                    Err(RecvError::Lagged(missed)) => {
                        warn!(missed, "drift detection fell behind the bus: looking at every node");
                        self.look_all();
                    }
                    Err(RecvError::Closed) => break,
                },
                _ = tick.tick() => {
                    self.reload(None).await;
                    self.look_all();
                }
            }
            self.publish().await;
        }
    }

    fn on_event(&mut self, event: &Event) {
        match event {
            Event::Facts {
                node,
                facts,
                changed,
            } if changed.contains(&Set::Config) => {
                self.hashes.insert(*node, facts.config.clone());
                self.look(*node);
            }
            Event::Removed { node } => self.forget(*node),
            _ => {}
        }
    }

    async fn on_job(&mut self, job: &Job) {
        let nodes: Vec<NodeId> = job
            .node
            .into_iter()
            .chain(job.nodes.iter().map(|n| n.node))
            .collect();
        if job.state == JobState::Running {
            self.jobs.insert(job.id, nodes);
            return;
        }
        if self.jobs.remove(&job.id).is_none() {
            return;
        }
        for node in nodes {
            if !self.busy(node) {
                self.reload(Some(node)).await;
                self.collect(node);
            }
        }
    }

    async fn on_command(&mut self, command: Command) {
        match command {
            Command::Acknowledge {
                node,
                package,
                by,
                done,
            } => {
                let _ = done.send(self.acknowledge(node, package, by).await);
            }
            Command::Mute {
                node,
                package,
                muted,
                by,
                done,
            } => {
                let _ = done.send(self.mute(node, package, muted, by).await);
            }
            Command::Read { node, readings } => self.on_read(node, readings).await,
            Command::Hashes { node, hashes } => {
                if let Some(hashes) = hashes {
                    self.hashes.insert(node, hashes);
                }
                self.look(node);
            }
        }
    }

    fn now(&self) -> Timestamp {
        self.engine.context().clock.now()
    }

    fn busy(&self, node: NodeId) -> bool {
        self.jobs.values().any(|nodes| nodes.contains(&node))
    }

    fn look_all(&mut self) {
        let nodes: Vec<NodeId> = self.hashes.keys().copied().collect();
        for node in nodes {
            self.look(node);
        }
    }

    fn snapshot(&self) -> DriftState {
        DriftState {
            drifts: self.views.values().cloned().collect(),
            muted: self.mutes.values().cloned().collect(),
        }
    }

    /// Forgets the drifts that ended in the store, then announces the state when it changed.
    async fn publish(&mut self) {
        self.flush().await;
        let state = self.snapshot();
        let changes = std::mem::take(&mut self.changes);
        if changes.is_empty() && **self.state.borrow() == state {
            return;
        }
        let state = Arc::new(state);
        self.state.send_replace(Arc::clone(&state));
        self.engine.context().publish(Event::Drift {
            state,
            changes: changes.into(),
        });
    }
}

async fn next_job(jobs: &mut Option<broadcast::Receiver<Arc<Job>>>) -> Option<Arc<Job>> {
    let rx = jobs.as_mut()?;
    loop {
        match rx.recv().await {
            Ok(job) => return Some(job),
            Err(RecvError::Lagged(missed)) => {
                warn!(missed, "drift detection fell behind the jobs");
            }
            Err(RecvError::Closed) => {
                *jobs = None;
                return None;
            }
        }
    }
}
