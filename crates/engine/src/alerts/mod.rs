//! Alerting (features/observability.md, Alerts; `uniwrt_events::alerts`): an evaluator fed by
//! the bus (recorded events, metric windows) and by the job runner (maintenance while jobs run),
//! ticked for alerts that resolve with time, its writes stored through [`AlertRepo`] and
//! announced as [`Event::Alerts`]; retention runs beside it.
//!
//! At start it stores the built-in rules not stored yet, and resumes the alerts still firing
//! (and those within their cool-down). The API changes rules and maintenance windows in the
//! store and then [`Alerts::reload`]s; acknowledgements go through [`Alerts::acknowledge`], so
//! that every change to an alert is announced in the order it was stored.

use std::{sync::Arc, time::Duration};

use tokio::{
    sync::{
        broadcast::{self, error::RecvError},
        mpsc, oneshot,
    },
    task::JoinHandle,
};
use tracing::warn;
use uniwrt_core::{AlertId, Timestamp};
use uniwrt_events::{AlertWrite, Evaluator, presets};
use uniwrt_model::{
    alerts::{Acknowledgement, AlertRecord, MAX_WINDOW_S},
    job::{Job, JobState},
};
use uniwrt_node::NodeLink;
use uniwrt_store::AlertRepo;

use crate::{Engine, Event, engine::Context};

#[derive(Debug, Clone, Copy)]
pub struct AlertsConfig {
    pub evaluate: uniwrt_events::AlertsConfig,
    /// How often alerts that resolve with time are looked at.
    pub tick_every: Duration,
    /// Resolved alerts are kept this long.
    pub retention: Duration,
    pub prune_every: Duration,
}

impl Default for AlertsConfig {
    fn default() -> Self {
        Self {
            evaluate: uniwrt_events::AlertsConfig::default(),
            tick_every: Duration::from_secs(5),
            retention: Duration::from_hours(90 * 24),
            prune_every: Duration::from_hours(1),
        }
    }
}

/// The API's way to the alert engine. Clones share it.
#[derive(Debug, Clone)]
pub struct Alerts {
    commands: mpsc::Sender<Command>,
}

/// The alert engine stopped, or could not store the change.
#[derive(Debug, thiserror::Error)]
pub enum AlertsError {
    #[error("the alert engine is not running")]
    Stopped,
    #[error("cannot store the alert: {0}")]
    Store(#[from] uniwrt_store::Error),
}

#[derive(Debug)]
enum Command {
    Reload(oneshot::Sender<()>),
    Acknowledge {
        id: AlertId,
        ack: Acknowledgement,
        reply: oneshot::Sender<Result<Option<AlertRecord>, uniwrt_store::Error>>,
    },
}

impl Alerts {
    /// Reads the rules and maintenance windows again, once they changed in the store; returns
    /// once they apply.
    pub async fn reload(&self) -> Result<(), AlertsError> {
        let (done, applied) = oneshot::channel();
        self.commands
            .send(Command::Reload(done))
            .await
            .map_err(|_| AlertsError::Stopped)?;
        applied.await.map_err(|_| AlertsError::Stopped)
    }

    /// Acknowledges an alert that still fires; `None` when there is no such alert or it
    /// resolved.
    pub async fn acknowledge(
        &self,
        id: AlertId,
        ack: Acknowledgement,
    ) -> Result<Option<AlertRecord>, AlertsError> {
        let (reply, replied) = oneshot::channel();
        self.commands
            .send(Command::Acknowledge { id, ack, reply })
            .await
            .map_err(|_| AlertsError::Stopped)?;
        Ok(replied.await.map_err(|_| AlertsError::Stopped)??)
    }
}

/// Evaluates alert rules until stopped.
#[derive(Debug)]
pub struct AlertEngine {
    stop: oneshot::Sender<()>,
    task: JoinHandle<()>,
    handle: Alerts,
}

impl AlertEngine {
    /// Starts evaluating what the engine announces, and silencing while `jobs` run.
    pub async fn start<L: NodeLink, S: AlertRepo + Clone + 'static>(
        engine: &Engine<L>,
        repo: S,
        jobs: Option<broadcast::Receiver<Arc<Job>>>,
        config: AlertsConfig,
    ) -> Self {
        let bus = engine.subscribe();
        let ctx = Arc::clone(engine.context());
        let now = ctx.clock.now();
        match presets() {
            Ok(presets) => {
                let presets = presets.into_iter().map(|p| (p.id, p.spec)).collect();
                if let Err(err) = repo.seed_alert_rules(presets).await {
                    warn!(%err, "cannot store the built-in alert rules");
                }
            }
            Err(err) => warn!(%err, "the built-in alert rules do not load"),
        }
        let since = now.saturating_sub(Duration::from_secs(MAX_WINDOW_S.into()));
        let restored = repo.alerts_to_resume(since).await.unwrap_or_else(|err| {
            warn!(%err, "cannot read the alerts to resume: they start afresh");
            Vec::new()
        });
        let mut task = Task {
            ctx,
            repo,
            evaluator: Evaluator::new(config.evaluate, restored, now),
        };
        let writes = task.reload().await;
        task.store(writes).await;
        let (commands, received) = mpsc::channel(16);
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(task.run(bus, jobs, received, config, stopped));
        Self {
            stop,
            task,
            handle: Alerts { commands },
        }
    }

    pub fn handle(&self) -> Alerts {
        self.handle.clone()
    }

    pub async fn stop(self) {
        let _ = self.stop.send(());
        let _ = self.task.await;
    }
}

struct Task<S> {
    ctx: Arc<Context>,
    repo: S,
    evaluator: Evaluator,
}

impl<S: AlertRepo> Task<S> {
    async fn run(
        mut self,
        mut bus: broadcast::Receiver<Event>,
        mut jobs: Option<broadcast::Receiver<Arc<Job>>>,
        mut commands: mpsc::Receiver<Command>,
        config: AlertsConfig,
        mut stopped: oneshot::Receiver<()>,
    ) {
        let mut tick = tokio::time::interval(config.tick_every);
        let mut prune = tokio::time::interval(config.prune_every);
        for timer in [&mut tick, &mut prune] {
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        }
        loop {
            let writes = tokio::select! {
                event = bus.recv() => match event {
                    Ok(event) => self.on_event(&event),
                    Err(RecvError::Lagged(missed)) => {
                        warn!(missed, "the alert engine fell behind the bus: alerts may be missed");
                        Vec::new()
                    }
                    Err(RecvError::Closed) => break,
                },
                Some(job) = next_job(&mut jobs) => {
                    if let Some(node) = job.node {
                        let at = job.finished_at.unwrap_or_else(|| self.now());
                        let running = job.state == JobState::Running;
                        self.evaluator.silences().job(node, running, at);
                    }
                    Vec::new()
                }
                Some(command) = commands.recv() => self.command(command).await,
                _ = tick.tick() => {
                    let now = self.now();
                    self.evaluator.tick(now)
                }
                _ = prune.tick() => {
                    let before = self.now().saturating_sub(config.retention);
                    if let Err(err) = self.repo.prune_alerts(before).await {
                        warn!(%err, "cannot prune the alerts");
                    }
                    Vec::new()
                }
                _ = &mut stopped => break,
            };
            self.store(writes).await;
        }
    }

    fn now(&self) -> Timestamp {
        self.ctx.clock.now()
    }

    fn on_event(&mut self, event: &Event) -> Vec<AlertWrite> {
        match event {
            Event::Recorded { events } => {
                let now = self.now();
                self.evaluator.events(events, now)
            }
            Event::Sampled { samples } => self.evaluator.windows(samples),
            _ => Vec::new(),
        }
    }

    async fn command(&mut self, command: Command) -> Vec<AlertWrite> {
        match command {
            Command::Reload(done) => {
                let writes = self.reload().await;
                let _ = done.send(());
                writes
            }
            Command::Acknowledge { id, ack, reply } => {
                let acked = self.repo.acknowledge_alert(id, ack).await;
                if let Ok(Some(alert)) = &acked {
                    self.ctx.publish(Event::Alerts {
                        alerts: Arc::from([alert.clone()]),
                    });
                }
                let _ = reply.send(acked);
                Vec::new()
            }
        }
    }

    /// The rules and windows from the store.
    async fn reload(&mut self) -> Vec<AlertWrite> {
        let now = self.now();
        match self.repo.maintenance_windows(now).await {
            Ok(windows) => self.evaluator.silences().set_windows(windows),
            Err(err) => warn!(%err, "cannot read the maintenance windows"),
        }
        match self.repo.alert_rules().await {
            Ok(rules) => self.evaluator.set_rules(rules, now),
            Err(err) => {
                warn!(%err, "cannot read the alert rules");
                Vec::new()
            }
        }
    }

    async fn store(&self, writes: Vec<AlertWrite>) {
        if writes.is_empty() {
            return;
        }
        match self.repo.record_alerts(writes).await {
            Ok(alerts) if !alerts.is_empty() => self.ctx.publish(Event::Alerts {
                alerts: alerts.into(),
            }),
            Ok(_) => {}
            Err(err) => warn!(%err, "cannot store the alerts"),
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
            Err(RecvError::Lagged(_)) => {}
            Err(RecvError::Closed) => {
                *jobs = None;
                return std::future::pending().await;
            }
        }
    }
}

#[cfg(test)]
mod tests;
