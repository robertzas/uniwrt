//! The neighbor reports' task: what each node holds, what to send it, and what to announce.

use std::{collections::BTreeMap, sync::Arc};

use tokio::sync::{
    broadcast::{self, error::RecvError},
    mpsc, oneshot, watch,
};
use tracing::{debug, warn};
use uniwrt_core::{JobId, NodeId, RevisionId, Timestamp};
use uniwrt_model::{
    events::{EventKind, NewEvent},
    job::{Job, JobState},
    site::{SiteConfig, node::ManagedPackage},
    status::NodeState,
};
use uniwrt_node::{
    Error as LinkError, NodeLink,
    proto::ops::{NeighborReport, OwnReport, Rrm, RrmState},
};
use uniwrt_store::RevisionRepo;

use super::{
    NeighborsConfig, OwnReports,
    plan::{self, Bss, Heard},
};
use crate::{Engine, Event, wlans::next_job};

type NodeLists = BTreeMap<String, Vec<NeighborReport>>;

/// What the controller knows of a node's BSSes.
#[derive(Default)]
struct NodeReports {
    own: Vec<OwnReport>,
    /// The lists it took last; `None` until it took any, or once it may have lost them.
    took: Option<NodeLists>,
    running: bool,
    /// Its lists changed while a request ran.
    again: bool,
    /// When its latest request failed (its condition is raised).
    failed: Option<Timestamp>,
}

/// An `rrm` request that ended.
struct Done {
    node: NodeId,
    lists: NodeLists,
    result: Result<RrmState, String>,
}

pub(super) struct Task<L, R> {
    engine: Engine<L>,
    repo: R,
    config: NeighborsConfig,
    head: Option<(RevisionId, Arc<SiteConfig>)>,
    /// The nodes each running job holds.
    jobs: BTreeMap<JobId, Vec<NodeId>>,
    heard: Heard,
    nodes: BTreeMap<NodeId, NodeReports>,
    done: (mpsc::Sender<Done>, mpsc::Receiver<Done>),
    own: watch::Sender<Arc<OwnReports>>,
}

impl<L, R> Task<L, R>
where
    L: NodeLink,
    R: RevisionRepo + Clone + 'static,
{
    pub(super) fn new(
        engine: Engine<L>,
        repo: R,
        config: NeighborsConfig,
        own: watch::Sender<Arc<OwnReports>>,
    ) -> Self {
        Self {
            engine,
            repo,
            config,
            head: None,
            jobs: BTreeMap::new(),
            heard: Heard::new(),
            nodes: BTreeMap::new(),
            done: mpsc::channel(32),
            own,
        }
    }

    /// Shares the own reports when they changed.
    fn publish(&self) {
        let own: OwnReports = self
            .nodes
            .iter()
            .filter(|(_, reports)| !reports.own.is_empty())
            .map(|(&node, reports)| (node, reports.own.clone()))
            .collect();
        self.own.send_if_modified(|shared| {
            let changed = **shared != own;
            if changed {
                *shared = Arc::new(own);
            }
            changed
        });
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
                    Ok(Event::Status { node, status }) => {
                        if status.state == NodeState::Offline {
                            // Its BSSes leave the others' lists; back, it may have restarted.
                            if let Some(reports) = self.nodes.get_mut(&node) {
                                reports.own.clear();
                                reports.took = None;
                            }
                            self.publish();
                        }
                        self.compare(false);
                    }
                    Ok(Event::Removed { node }) => {
                        self.nodes.remove(&node);
                        self.publish();
                        self.compare(false);
                    }
                    Ok(Event::Rf { view }) => {
                        let heard = plan::heard(&view.report);
                        if heard != self.heard {
                            self.heard = heard;
                            self.compare(false);
                        }
                    }
                    Ok(_) | Err(RecvError::Lagged(_)) => {}
                    Err(RecvError::Closed) => break,
                },
                _ = tick.tick() => {
                    self.reload().await;
                    self.compare(true);
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
            Err(err) => warn!(%err, "neighbor reports: cannot read the site"),
        }
    }

    fn on_job(&mut self, job: &Job) {
        if job.state == JobState::Running {
            let nodes = job.node.into_iter().chain(job.nodes.iter().map(|n| n.node));
            self.jobs.insert(job.id, nodes.collect());
        } else if let Some(nodes) = self.jobs.remove(&job.id) {
            // What the job left (an apply's `wifi reload` restarts BSSes) gets its lists again.
            for node in nodes {
                if let Some(reports) = self.nodes.get_mut(&node) {
                    reports.took = None;
                }
            }
            self.compare(false);
        }
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

    /// The node's BSSes whose lists usteer sets (every second, D-090): those of the steered
    /// SSIDs where usteer runs. They are left to it.
    fn usteer_owns(&self, node: NodeId) -> Vec<String> {
        let Some((_, site)) = &self.head else {
            return Vec::new();
        };
        let facts = self.engine.node(node).and_then(|view| view.facts);
        let config = site.nodes.iter().find(|n| n.node == node);
        let (Some(facts), Some(config)) = (facts, config) else {
            return Vec::new();
        };
        if !site.steers(config, &facts) {
            return Vec::new();
        }
        let ssids = site.steered_ssids();
        self.nodes.get(&node).map_or_else(Vec::new, |reports| {
            reports
                .own
                .iter()
                .filter(|r| ssids.contains(&r.ssid.as_str()))
                .map(|r| r.ifname.clone())
                .collect()
        })
    }

    fn busy(&self, node: NodeId) -> bool {
        self.jobs.values().any(|nodes| nodes.contains(&node))
    }

    /// The managed nodes' BSSes known, each with its band from its node's facts.
    fn bsses(&self, managed: &[NodeId]) -> Vec<Bss> {
        let mut bsses = Vec::new();
        for &node in managed {
            let Some(reports) = self.nodes.get(&node) else {
                continue;
            };
            let facts = self.engine.node(node).and_then(|view| view.facts);
            for report in &reports.own {
                let band = facts.as_ref().and_then(|facts| {
                    facts.radios.iter().find_map(|radio| {
                        let named = |i: &uniwrt_model::facts::WifiIfaceFacts| {
                            i.name.as_deref() == Some(report.ifname.as_str())
                        };
                        radio.ifaces.iter().any(named).then_some(radio.band)
                    })
                });
                bsses.push(Bss {
                    node,
                    band,
                    report: report.clone(),
                });
            }
        }
        bsses
    }

    /// Sends the managed nodes the lists they don't hold; `all` sends every node its lists
    /// (the agent leaves alone BSSes that hold them), which also learns their own reports.
    fn compare(&mut self, all: bool) {
        let now = self.engine.context().clock.now();
        let managed = self.managed();
        let mut lists = plan::lists(
            &self.bsses(&managed),
            &self.heard,
            self.config.max_neighbors,
        );
        for node in managed {
            if self.busy(node) {
                continue;
            }
            let mut wanted = lists.remove(&node).unwrap_or_default();
            let usteer = self.usteer_owns(node);
            wanted.retain(|ifname, _| !usteer.contains(ifname));
            let reports = self.nodes.entry(node).or_default();
            if reports.running {
                reports.again |= reports.took.as_ref() != Some(&wanted);
                continue;
            }
            let retry =
                |failed: Timestamp| now.saturating_duration_since(failed) < self.config.retry;
            if reports.failed.is_some_and(retry) || (!all && reports.took.as_ref() == Some(&wanted))
            {
                continue;
            }
            self.send(node, wanted);
        }
    }

    fn send(&mut self, node: NodeId, lists: NodeLists) {
        let Some(link) = self.engine.link(node) else {
            return;
        };
        let reports = self.nodes.entry(node).or_default();
        reports.running = true;
        reports.again = false;
        debug!(%node, bsses = lists.len(), "neighbor reports: sending");
        let (done, timeout) = (self.done.0.clone(), self.config.timeout);
        tokio::spawn(async move {
            let rrm = Rrm {
                lists: lists.clone(),
            };
            let result = link.request(&rrm, timeout).await.map_err(|err| match err {
                // What the node said, for people; the code is for the controller.
                LinkError::Remote { msg, .. } => msg,
                other => other.to_string(),
            });
            let _ = done
                .send(Done {
                    node,
                    lists,
                    result,
                })
                .await;
        });
    }

    fn on_done(
        &mut self,
        Done {
            node,
            lists,
            result,
        }: Done,
    ) {
        let now = self.engine.context().clock.now();
        let Some(reports) = self.nodes.get_mut(&node) else {
            return;
        };
        reports.running = false;
        let mut changed = false;
        let error = match result {
            Ok(state) => {
                changed = state.own != reports.own;
                reports.own = state.own;
                // A BSS gone meanwhile isn't a refusal: it left the own reports too.
                let refused = state
                    .failed
                    .into_iter()
                    .find(|f| reports.own.iter().any(|o| o.ifname == f.ifname));
                match refused {
                    None => {
                        reports.took = Some(lists);
                        None
                    }
                    Some(f) => Some(format!("{}: {}", f.ifname, f.error)),
                }
            }
            Err(error) => Some(error),
        };
        let condition = format!("node:{node}/neighbor-reports");
        match error {
            None => {
                if reports.failed.take().is_some() {
                    let event = NewEvent::new(now, EventKind::NeighborReportsOk).node(node);
                    self.engine.note(event.condition(condition, false));
                }
            }
            Some(error) => {
                warn!(%node, %error, "neighbor reports: the node refused");
                reports.took = None;
                if reports.failed.replace(now).is_none() {
                    let what = EventKind::NeighborReportsFailed { error };
                    let event = NewEvent::new(now, what).node(node);
                    self.engine
                        .note(event.condition(condition.clone(), true).fold(condition));
                }
            }
        }
        let again = std::mem::take(&mut reports.again);
        if changed {
            self.publish();
        }
        if changed || again {
            self.compare(false);
        }
    }
}
