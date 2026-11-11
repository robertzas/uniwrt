//! The schedules' task: when to compare, what to send, and what to announce.

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
    job::{Job, JobState},
    site::{SiteConfig, tz},
    status::NodeState,
};
use uniwrt_node::{
    Error as LinkError, NodeLink,
    proto::ops::{Bss, Set},
};
use uniwrt_store::RevisionRepo;

use super::{SchedulesConfig, plan};
use crate::{Engine, Event};

/// A node's phy, which the `bss` op acts on.
type Key = (NodeId, String);

/// What was last sent to a phy, and when.
struct Sent {
    at: Timestamp,
    off: Vec<String>,
}

/// A `bss` request that ended.
struct Done {
    key: Key,
    result: Result<(), String>,
}

pub(super) struct Task<L, R> {
    engine: Engine<L>,
    repo: R,
    config: SchedulesConfig,
    head: Option<(RevisionId, Arc<SiteConfig>)>,
    /// The nodes each running job holds.
    jobs: BTreeMap<JobId, Vec<NodeId>>,
    sent: BTreeMap<Key, Sent>,
    /// The phys a request runs on now.
    running: BTreeSet<Key>,
    /// The phys whose latest request failed (their condition is raised).
    failing: BTreeSet<Key>,
    /// Whether each scheduled WLAN's schedule had it on, as last announced.
    wlans: BTreeMap<String, bool>,
    done: (mpsc::Sender<Done>, mpsc::Receiver<Done>),
}

impl<L, R> Task<L, R>
where
    L: NodeLink,
    R: RevisionRepo + Clone + 'static,
{
    pub(super) fn new(engine: Engine<L>, repo: R, config: SchedulesConfig) -> Self {
        Self {
            engine,
            repo,
            config,
            head: None,
            jobs: BTreeMap::new(),
            sent: BTreeMap::new(),
            running: BTreeSet::new(),
            failing: BTreeSet::new(),
            wlans: BTreeMap::new(),
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
                    Ok(Event::Facts { node, changed, .. }) if changed.contains(&Set::Radios) => {
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
            Err(err) => warn!(%err, "WLAN schedules: cannot read the site"),
        }
    }

    fn on_job(&mut self, job: &Job) {
        if job.state == JobState::Running {
            let nodes = job.node.into_iter().chain(job.nodes.iter().map(|n| n.node));
            self.jobs.insert(job.id, nodes.collect());
        } else if let Some(nodes) = self.jobs.remove(&job.id) {
            // What the job left (an apply's `wifi reload` brings every BSS back) is compared
            // anew at once.
            for node in nodes {
                self.sent.retain(|(n, _), _| *n != node);
                self.compare(Some(node));
            }
        }
    }

    fn busy(&self, node: NodeId) -> bool {
        self.jobs.values().any(|nodes| nodes.contains(&node))
    }

    /// Compares the nodes (or `only`) with their schedules and acts where they differ; on a full
    /// round, announces the WLANs whose schedules turned.
    fn compare(&mut self, only: Option<NodeId>) {
        let Some((_, config)) = self.head.clone() else {
            return;
        };
        let now = self.engine.context().clock.now();
        if only.is_none() {
            self.announce(&config, now);
        }
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
            let Some(facts) = view.facts else { continue };
            if view.status.state == NodeState::Offline || self.busy(node) {
                continue;
            }
            let bsses = plan::bsses(&config, node, &facts.radios, now);
            let mut phys: BTreeMap<&str, Vec<&plan::Bss>> = BTreeMap::new();
            for bss in &bsses {
                if let Some(phy) = bss.phy.as_deref() {
                    phys.entry(phy).or_default().push(bss);
                }
            }
            for (phy, bsses) in phys {
                self.act(node, phy, &bsses, now);
            }
        }
    }

    /// Sends a phy the BSSes to keep off, when its BSSes aren't as their schedules want.
    fn act(&mut self, node: NodeId, phy: &str, bsses: &[&plan::Bss], now: Timestamp) {
        let wrong = bsses
            .iter()
            .any(|b| (!b.on && b.up) || (b.on && !b.up && b.radio_up));
        let key = (node, phy.to_owned());
        if !wrong || self.running.contains(&key) {
            return;
        }
        let mut off: Vec<String> = bsses
            .iter()
            .filter(|b| !b.on)
            .filter_map(|b| b.ifname.clone())
            .collect();
        off.sort();
        if let Some(sent) = self.sent.get(&key) {
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
        debug!(%node, phy, ?off, "WLAN schedules: setting the BSSes");
        self.sent.insert(
            key.clone(),
            Sent {
                at: now,
                off: off.clone(),
            },
        );
        self.running.insert(key.clone());
        let request = Bss {
            phy: phy.to_owned(),
            off,
        };
        let (done, timeout) = (self.done.0.clone(), self.config.timeout);
        tokio::spawn(async move {
            let result =
                link.request(&request, timeout)
                    .await
                    .map(|_| ())
                    .map_err(|err| match err {
                        // What the node said, for people; the code is for the controller.
                        LinkError::Remote { msg, .. } => msg,
                        other => other.to_string(),
                    });
            let _ = done.send(Done { key, result }).await;
        });
    }

    fn on_done(&mut self, Done { key, result }: Done) {
        self.running.remove(&key);
        let (node, phy) = &key;
        let at = self.engine.context().clock.now();
        let condition = format!("node:{node}/wlan-schedule:{phy}");
        match result {
            Ok(()) => {
                if self.failing.remove(&key) {
                    let what = EventKind::WlanScheduleOk { phy: phy.clone() };
                    let event = NewEvent::new(at, what).node(*node);
                    self.engine.note(event.condition(condition, false));
                }
            }
            Err(error) => {
                warn!(%node, phy, %error, "WLAN schedules: the node refused");
                if self.failing.insert(key.clone()) {
                    let what = EventKind::WlanScheduleFailed {
                        phy: phy.clone(),
                        error,
                    };
                    let event = NewEvent::new(at, what).node(*node);
                    self.engine
                        .note(event.condition(condition.clone(), true).fold(condition));
                }
            }
        }
    }

    /// Announces the WLANs whose schedules turned since the last round. The first round only
    /// learns where they stand: a controller that starts doesn't announce what was already so.
    fn announce(&mut self, config: &SiteConfig, now: Timestamp) {
        let Some(local) = tz::posix(&config.site.timezone).and_then(|p| tz::local(p, now)) else {
            return;
        };
        let first = self.wlans.is_empty();
        let mut seen = BTreeMap::new();
        for wlan in config.wlans.iter().filter(|w| w.enabled) {
            let Some(schedule) = &wlan.schedule else {
                continue;
            };
            let on = schedule.is_on(local.day, local.minute);
            if !first && self.wlans.get(&wlan.id).is_some_and(|was| *was != on) {
                let what = EventKind::WlanScheduled {
                    wlan: wlan.id.clone(),
                    ssid: wlan.ssid.clone(),
                    on,
                };
                self.engine.note(NewEvent::new(now, what));
            }
            seen.insert(wlan.id.clone(), on);
        }
        self.wlans = seen;
    }
}

pub(crate) async fn next_job(jobs: &mut Option<broadcast::Receiver<Arc<Job>>>) -> Option<Arc<Job>> {
    let rx = jobs.as_mut()?;
    loop {
        match rx.recv().await {
            Ok(job) => return Some(job),
            Err(RecvError::Lagged(missed)) => {
                warn!(missed, "fell behind the jobs");
            }
            Err(RecvError::Closed) => {
                *jobs = None;
                return None;
            }
        }
    }
}
