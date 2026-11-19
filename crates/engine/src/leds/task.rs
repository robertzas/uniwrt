//! The LEDs schedule's task: when to compare, what to send, and what to raise.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use tokio::sync::{
    broadcast::{self, error::RecvError},
    mpsc, oneshot,
};
use tracing::{debug, warn};
use uniwrt_core::{JobId, NodeId, RevisionId, Timestamp};
use uniwrt_model::{
    events::{EventKind, NewEvent},
    facts::LedFacts,
    job::{Job, JobState},
    site::{SiteConfig, tz},
    status::NodeState,
};
use uniwrt_node::{
    Error as LinkError, NodeLink,
    proto::ops::{Leds, Set},
};
use uniwrt_store::RevisionRepo;

use super::LedScheduleConfig;
use crate::{Engine, Event, wlans::next_job};

/// What was last sent to a node, and when.
struct Sent {
    at: Timestamp,
    off: bool,
}

/// A `leds` request that ended.
struct Done {
    node: NodeId,
    result: Result<(), String>,
}

pub(super) struct Task<L, R> {
    engine: Engine<L>,
    repo: R,
    config: LedScheduleConfig,
    head: Option<(RevisionId, Arc<SiteConfig>)>,
    /// The nodes each running job holds.
    jobs: BTreeMap<JobId, Vec<NodeId>>,
    sent: BTreeMap<NodeId, Sent>,
    /// The nodes a request runs on now.
    running: BTreeSet<NodeId>,
    /// The nodes whose latest request failed (their condition is raised).
    failing: BTreeSet<NodeId>,
    done: (mpsc::Sender<Done>, mpsc::Receiver<Done>),
}

/// What a node's LEDs need, if anything: `Some(true)` to go dark, `Some(false)` to come back.
/// Only a managed node (one whose packages the controller owns) is touched.
pub(super) fn wanted(
    config: &SiteConfig,
    node: NodeId,
    leds: &LedFacts,
    local: tz::Local,
) -> Option<bool> {
    let managed = config
        .nodes
        .iter()
        .any(|n| n.node == node && !n.managed.is_empty());
    if !managed || leds.count == 0 {
        return None;
    }
    let off = config.leds_off(node, local.day, local.minute);
    match (off, leds.lit > 0, leds.held) {
        (true, true, _) => Some(true),
        (false, _, true) => Some(false),
        _ => None,
    }
}

impl<L, R> Task<L, R>
where
    L: NodeLink,
    R: RevisionRepo + Clone + 'static,
{
    pub(super) fn new(engine: Engine<L>, repo: R, config: LedScheduleConfig) -> Self {
        Self {
            engine,
            repo,
            config,
            head: None,
            jobs: BTreeMap::new(),
            sent: BTreeMap::new(),
            running: BTreeSet::new(),
            failing: BTreeSet::new(),
            done: mpsc::channel(16),
        }
    }

    pub(super) async fn run(
        mut self,
        mut bus: broadcast::Receiver<Event>,
        mut jobs: Option<broadcast::Receiver<Arc<Job>>>,
        mut stopped: oneshot::Receiver<()>,
    ) {
        let mut tick = tokio::time::interval(self.config.tick_every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                biased;
                _ = &mut stopped => break,
                Some(job) = next_job(&mut jobs) => self.on_job(&job),
                Some(done) = self.done.1.recv() => self.on_done(done),
                event = bus.recv() => match event {
                    Ok(Event::Facts { node, changed, .. }) if changed.contains(&Set::System) => {
                        self.compare(Some(node));
                    }
                    Ok(_) | Err(RecvError::Lagged(_)) => {}
                    Err(RecvError::Closed) => break,
                },
                _ = tick.tick() => {
                    self.reload().await;
                    self.compare(None);
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
            Err(err) => warn!(%err, "LEDs schedule: cannot read the site"),
        }
    }

    fn on_job(&mut self, job: &Job) {
        if job.state == JobState::Running {
            let nodes = job.node.into_iter().chain(job.nodes.iter().map(|n| n.node));
            self.jobs.insert(job.id, nodes.collect());
        } else if let Some(nodes) = self.jobs.remove(&job.id) {
            // A job may have restarted the node (its LEDs came back): compared anew at once.
            for node in nodes {
                self.sent.remove(&node);
                self.compare(Some(node));
            }
        }
    }

    fn busy(&self, node: NodeId) -> bool {
        self.jobs.values().any(|nodes| nodes.contains(&node))
    }

    /// Compares the nodes (or `only`) with the schedule and acts where they differ.
    fn compare(&mut self, only: Option<NodeId>) {
        let Some((_, config)) = self.head.clone() else {
            return;
        };
        let now = self.engine.context().clock.now();
        let Some(local) = tz::posix(&config.site.timezone).and_then(|p| tz::local(p, now)) else {
            return;
        };
        let nodes = match only {
            Some(node) => self
                .engine
                .node(node)
                .map(|v| (node, v))
                .into_iter()
                .collect(),
            None => self.engine.all(),
        };
        for (node, view) in nodes {
            let Some(leds) = view.facts.as_ref().and_then(|f| f.system.leds) else {
                continue;
            };
            if view.status.state == NodeState::Offline || self.busy(node) {
                continue;
            }
            if let Some(off) = wanted(&config, node, &leds, local) {
                self.act(node, off, now);
            }
        }
    }

    fn act(&mut self, node: NodeId, off: bool, now: Timestamp) {
        if self.running.contains(&node) {
            return;
        }
        if let Some(sent) = self.sent.get(&node) {
            let wait = if sent.off == off {
                self.config.retry
            } else {
                self.config.settle
            };
            if now.saturating_duration_since(sent.at) < wait {
                return;
            }
        }
        let Some(link) = self.engine.link(node) else {
            return;
        };
        debug!(%node, off, "LEDs schedule: setting the LEDs");
        self.sent.insert(node, Sent { at: now, off });
        self.running.insert(node);
        let (done, timeout) = (self.done.0.clone(), self.config.timeout);
        tokio::spawn(async move {
            let result = link
                .request(&Leds { off }, timeout)
                .await
                .map(|_| ())
                .map_err(|err| match err {
                    // What the node said, for people; the code is for the controller.
                    LinkError::Remote { msg, .. } => msg,
                    other => other.to_string(),
                });
            let _ = done.send(Done { node, result }).await;
        });
    }

    fn on_done(&mut self, Done { node, result }: Done) {
        self.running.remove(&node);
        let at = self.engine.context().clock.now();
        let condition = format!("node:{node}/led-schedule");
        match result {
            Ok(()) => {
                if self.failing.remove(&node) {
                    let event = NewEvent::new(at, EventKind::LedScheduleOk).node(node);
                    self.engine.note(event.condition(condition, false));
                }
            }
            Err(error) => {
                warn!(%node, %error, "LEDs schedule: the node refused");
                if self.failing.insert(node) {
                    let event =
                        NewEvent::new(at, EventKind::LedScheduleFailed { error }).node(node);
                    self.engine
                        .note(event.condition(condition.clone(), true).fold(condition));
                }
            }
        }
    }
}
