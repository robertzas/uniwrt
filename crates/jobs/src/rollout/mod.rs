//! Rolling firmware upgrades (features/lifecycle.md, Firmware; D-015, D-124, D-125): several
//! managed nodes upgraded one at a time by c1's upgrade job each, leaves first and the gateway
//! last; an optional canary (the first node) soaked and checked before the rest; an optional
//! daily window outside which no node starts; pause, resume (a halted rollout retries its
//! failed node), skip a node, cancel.
//!
//! A rollout is a record ([`Rollout`] in the store), not a job: its driver ([`Rollouts::start`])
//! reads it, acts and saves it at every change, so it carries on after a controller restart (a
//! node's upgrade job interrupted by it fails, as jobs do (D-042), and halts the rollout on
//! that node). At most one rollout is active. A node's image is resolved at its turn through
//! [`RolloutImages`] (the API's sources: ASU's build waited for, the others at once), so its
//! plan is as fresh as the node.
//!
//! Every edit and every step of the driver loads, changes and saves the rollout under one lock;
//! the slow parts (an image readied, the canary checked) run outside it and re-check the record
//! before they save.

mod canary;
mod drive;
mod order;
#[cfg(test)]
mod tests;

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

use tokio::{
    sync::{Notify, broadcast},
    task::AbortHandle,
};
use tracing::warn;
use uniwrt_core::{NodeId, RolloutId, UserId};
use uniwrt_model::{
    facts::NodeFacts,
    node::NodeMode,
    rollout::{
        CANARY_SOAK_RANGE, Canary, NewRollout, Rollout, RolloutNode, RolloutNodeState,
        RolloutState, UpgradeChoice, UpgradeWindow,
    },
};
use uniwrt_store::{FactsRepo as _, NodeRepo as _, RolloutRepo as _};

use crate::{BoxFuture, Error, Jobs, Result, UpgradeRequest};

/// Readies a node's image for its upgrade (the API's image sources, D-122).
pub trait RolloutImages: Send + Sync + 'static {
    /// Asks for the image ahead of the node's turn (ASU starts building it); why not, for
    /// people.
    fn ahead<'a>(
        &'a self,
        node: NodeId,
        choice: &'a UpgradeChoice,
    ) -> BoxFuture<'a, Result<(), String>>;

    /// The node's upgrade with `choice`'s image, planned now, once the image is ready (ASU's
    /// build waited for); why not, for people.
    fn request<'a>(
        &'a self,
        node: NodeId,
        choice: &'a UpgradeChoice,
    ) -> BoxFuture<'a, Result<UpgradeRequest, String>>;
}

/// A rollout to start: each node's image, the canary and the window.
pub struct RolloutRequest {
    pub nodes: Vec<(NodeId, UpgradeChoice)>,
    /// Soak one node first (`node`, else the first in order); ignored for a single node.
    pub canary: Option<CanaryRequest>,
    pub window: Option<UpgradeWindow>,
    /// The adopted nodes' facts as the engine sees them now (for the order and each node's
    /// release); a node missing here is looked up in the store.
    pub facts: BTreeMap<NodeId, Arc<NodeFacts>>,
}

pub struct CanaryRequest {
    pub node: Option<NodeId>,
    pub soak_min: u32,
}

/// How often the driver looks again. [`Default`] is production's.
#[derive(Debug, Clone)]
pub struct RolloutTiming {
    /// When the next node is busy with another job.
    pub retry: Duration,
    /// At the latest, whatever it waits for.
    pub look: Duration,
}

impl Default for RolloutTiming {
    fn default() -> Self {
        Self {
            retry: Duration::from_secs(30),
            look: Duration::from_secs(60),
        }
    }
}

/// Rolling upgrades. Clones share them.
#[derive(Clone)]
pub struct Rollouts {
    inner: Arc<Inner>,
}

struct Inner {
    jobs: Jobs,
    timing: RolloutTiming,
    images: OnceLock<Arc<dyn RolloutImages>>,
    /// Held while a rollout is loaded, changed and saved.
    edit: tokio::sync::Mutex<()>,
    wake: Notify,
    updates: broadcast::Sender<Arc<Rollout>>,
    task: Mutex<Option<AbortHandle>>,
}

impl Rollouts {
    pub fn new(jobs: Jobs, timing: RolloutTiming) -> Self {
        Self {
            inner: Arc::new(Inner {
                jobs,
                timing,
                images: OnceLock::new(),
                edit: tokio::sync::Mutex::new(()),
                wake: Notify::new(),
                updates: broadcast::channel(64).0,
                task: Mutex::new(None),
            }),
        }
    }

    /// Starts the driver with the image sources: the active rollout, if any, carries on. Once.
    pub fn start(&self, images: Arc<dyn RolloutImages>) {
        if self.inner.images.set(images).is_err() {
            return;
        }
        let inner = Arc::clone(&self.inner);
        let task = tokio::spawn(async move { drive::run(inner).await });
        *self.task() = Some(task.abort_handle());
    }

    /// Stops the driver; the active rollout carries on at the next start.
    pub fn stop(&self) {
        if let Some(task) = self.task().take() {
            task.abort();
        }
    }

    /// Every change of a rollout, saved first.
    pub fn subscribe(&self) -> broadcast::Receiver<Arc<Rollout>> {
        self.inner.updates.subscribe()
    }

    /// The latest `limit` rollouts, newest first.
    pub async fn list(&self, limit: usize) -> Result<Vec<Rollout>> {
        Ok(self.store().rollouts(limit).await?)
    }

    pub async fn get(&self, id: RolloutId) -> Result<Option<Rollout>> {
        Ok(self.store().rollout(id).await?)
    }

    /// Starts a rollout: every node managed and once, the canary and window valid, no other
    /// rollout active. ASU is asked for its images at once.
    pub async fn create(&self, request: RolloutRequest, by: Option<UserId>) -> Result<Rollout> {
        if self.inner.jobs.shared.dial.is_none() || self.inner.images.get().is_none() {
            return Err(Error::Unavailable);
        }
        if request.nodes.is_empty() {
            return Err(Error::Refused("Choose at least one device".to_owned()));
        }
        if let Some(window) = request.window {
            window.check().map_err(Error::Refused)?;
        }
        if let Some(canary) = &request.canary {
            let (least, most) = CANARY_SOAK_RANGE;
            if !(least..=most).contains(&canary.soak_min) {
                return Err(Error::Refused(format!(
                    "The canary soaks {least} minutes to {} hours",
                    most / 60
                )));
            }
        }
        let mut seen = BTreeSet::new();
        if let Some((id, _)) = request.nodes.iter().find(|(id, _)| !seen.insert(*id)) {
            return Err(Error::Refused(format!("Node {id} is in the rollout twice")));
        }
        let _edit = self.inner.edit.lock().await;
        if let Some(active) = self.store().active_rollout().await? {
            return Err(Error::Refused(format!(
                "Rollout {} is still under way: finish or cancel it first",
                active.id
            )));
        }
        let mut adopted = Vec::new();
        for node in self.store().nodes().await? {
            let facts = match request.facts.get(&node.id) {
                Some(facts) => Some(NodeFacts::clone(facts)),
                None => self.store().latest_facts(node.id).await?,
            };
            adopted.push((node, facts));
        }
        let mut nodes = Vec::with_capacity(request.nodes.len());
        for (id, choice) in &request.nodes {
            let (node, facts) = adopted
                .iter()
                .find(|(node, _)| node.id == *id)
                .ok_or(Error::NoNode(*id))?;
            if node.mode != NodeMode::Managed {
                return Err(Error::Observed(*id));
            }
            nodes.push(RolloutNode {
                node: *id,
                name: node.name.clone(),
                choice: choice.clone(),
                from: facts
                    .as_ref()
                    .map(|facts| facts.board.release.version.clone())
                    .unwrap_or_default(),
                state: RolloutNodeState::Pending,
                job: None,
                detail: None,
            });
        }
        let chosen: Vec<NodeId> = nodes.iter().map(|node| node.node).collect();
        let several = chosen.len() > 1;
        let canary_node = request.canary.as_ref().and_then(|canary| canary.node);
        let order = order::sort(&adopted, &chosen, canary_node.filter(|_| several))
            .map_err(Error::Refused)?;
        nodes.sort_by_key(|node| order.iter().position(|&id| id == node.node));
        let canary = request.canary.filter(|_| several).map(|canary| Canary {
            soak_min: canary.soak_min,
        });
        let rollout = self
            .store()
            .create_rollout(NewRollout {
                nodes,
                canary,
                window: request.window,
                created_by: by,
            })
            .await?;
        self.announce(&rollout);
        self.ahead(&rollout);
        self.inner.wake.notify_one();
        Ok(rollout)
    }

    /// Pauses a running rollout: the node upgrading finishes, the next doesn't start.
    pub async fn pause(&self, id: RolloutId) -> Result<Rollout> {
        self.edit(id, |rollout| {
            if rollout.state != RolloutState::Running {
                return Err(not_now(rollout, "paused"));
            }
            rollout.state = RolloutState::Paused;
            Ok(())
        })
        .await
    }

    /// Resumes a paused rollout, or a halted one: its failed node is tried again.
    pub async fn resume(&self, id: RolloutId) -> Result<Rollout> {
        self.edit(id, |rollout| {
            if !matches!(rollout.state, RolloutState::Paused | RolloutState::Halted) {
                return Err(not_now(rollout, "resumed"));
            }
            for node in &mut rollout.nodes {
                if node.state == RolloutNodeState::Failed {
                    node.state = RolloutNodeState::Pending;
                    node.detail = Some("To be tried again".to_owned());
                }
            }
            rollout.state = RolloutState::Running;
            rollout.error = None;
            Ok(())
        })
        .await
    }

    /// Leaves a node out: one still to come, or the one a halted rollout failed on (resume
    /// goes on with the next).
    pub async fn skip(&self, id: RolloutId, node: NodeId) -> Result<Rollout> {
        self.edit(id, |rollout| {
            let Some(entry) = rollout.nodes.iter_mut().find(|entry| entry.node == node) else {
                return Err(Error::Refused(format!("Node {node} isn't in the rollout")));
            };
            if !matches!(
                entry.state,
                RolloutNodeState::Pending | RolloutNodeState::Failed
            ) {
                return Err(Error::Refused(format!(
                    "{} can't be skipped now: only a device still to come or failed can",
                    entry.name
                )));
            }
            entry.state = RolloutNodeState::Skipped;
            entry.detail = Some("Skipped: its firmware is as it was".to_owned());
            Ok(())
        })
        .await
    }

    /// Ends a rollout that no node's upgrade runs in: the nodes not upgraded keep their
    /// firmware.
    pub async fn cancel(&self, id: RolloutId) -> Result<Rollout> {
        self.edit(id, |rollout| {
            if !rollout.state.active() {
                return Err(not_now(rollout, "cancelled"));
            }
            if let Some(node) = rollout
                .nodes
                .iter()
                .find(|node| node.state == RolloutNodeState::Upgrading)
            {
                return Err(Error::Refused(format!(
                    "{} is upgrading: pause the rollout and cancel it once that ends",
                    node.name
                )));
            }
            for node in &mut rollout.nodes {
                if matches!(
                    node.state,
                    RolloutNodeState::Pending
                        | RolloutNodeState::Preparing
                        | RolloutNodeState::Failed
                ) {
                    node.state = RolloutNodeState::Skipped;
                    node.detail = Some("Cancelled: its firmware is as it was".to_owned());
                }
            }
            rollout.state = RolloutState::Cancelled;
            rollout.waiting = None;
            Ok(())
        })
        .await
    }

    /// Loads an active rollout, changes it with `change`, saves and announces it.
    async fn edit(
        &self,
        id: RolloutId,
        change: impl FnOnce(&mut Rollout) -> Result<()>,
    ) -> Result<Rollout> {
        let _edit = self.inner.edit.lock().await;
        let mut rollout = self
            .store()
            .rollout(id)
            .await?
            .ok_or(Error::NoRollout(id))?;
        change(&mut rollout)?;
        if rollout.state == RolloutState::Cancelled {
            rollout.finished_at = Some(self.inner.jobs.shared.clock.now());
        }
        self.store().update_rollout(&rollout).await?;
        self.announce(&rollout);
        self.inner.wake.notify_one();
        Ok(rollout)
    }

    /// Asks for ASU's images in the background, one after the other.
    fn ahead(&self, rollout: &Rollout) {
        let Some(images) = self.inner.images.get().cloned() else {
            return;
        };
        let asked: Vec<(NodeId, UpgradeChoice)> = rollout
            .nodes
            .iter()
            .filter(|node| matches!(node.choice, UpgradeChoice::Asu { .. }))
            .map(|node| (node.node, node.choice.clone()))
            .collect();
        tokio::spawn(async move {
            for (node, choice) in asked {
                if let Err(why) = images.ahead(node, &choice).await {
                    warn!(%node, %why, "rollout: cannot ask for the image ahead");
                }
            }
        });
    }

    fn announce(&self, rollout: &Rollout) {
        let _ = self.inner.updates.send(Arc::new(rollout.clone()));
    }

    fn store(&self) -> &uniwrt_store::Store {
        &self.inner.jobs.shared.store
    }

    fn task(&self) -> std::sync::MutexGuard<'_, Option<AbortHandle>> {
        self.inner
            .task
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn not_now(rollout: &Rollout, what: &str) -> Error {
    let state = match rollout.state {
        RolloutState::Running => "running",
        RolloutState::Paused => "paused",
        RolloutState::Halted => "halted",
        RolloutState::Done => "done",
        RolloutState::Cancelled => "cancelled",
    };
    Error::Refused(format!(
        "Rollout {} is {state}: it can't be {what}",
        rollout.id
    ))
}
