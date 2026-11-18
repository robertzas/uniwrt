//! Taking a node out of the site (features/lifecycle.md, Replace / RMA / decommission; D-131).
//!
//! - **Decommission:** connect → remove the transaction tool (refused while a transaction is
//!   open; the node is in observe mode from then on) → wipe (the controller stops watching it,
//!   then [`uniwrt_node::decommission::wipe`] runs detached on it: the mesh left, the uniwrt
//!   footprint and the controller's key removed, `firstboot` and a restart when asked) →
//!   record ([`forget`]). A failure watches it again (offline, once the wipe has run).
//! - **Forget:** record alone, for a node that can't be reached: nothing changes on it.
//!
//! Both hold the node and the site's configuration (no apply runs meanwhile), and are refused
//! for the gateway while other nodes are adopted (they depend on it: replace it instead) and
//! for a node an active rollout still has to upgrade.

mod forget;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use tracing::{info, warn};
use uniwrt_core::{NodeId, UserId};
use uniwrt_model::{
    job::{Job, JobError, JobErrorCode, JobKind, NewJob, StepName},
    node::{Node, NodeMode, NodeRole},
    rollout::RolloutNodeState,
};
use uniwrt_node::{HostKey, decommission};
use uniwrt_store::{HostKeyRepo as _, NodeRepo as _, RolloutRepo as _};

use crate::{
    Error, Jobs, Remote, Result, apply,
    errors::{internal, node_error},
    manage,
    progress::Progress,
    runner::Shared,
};

/// Who asked, for the revision that leaves the node out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Author {
    pub id: UserId,
    pub name: String,
}

const DECOMMISSION: [StepName; 4] = [
    StepName::Connect,
    StepName::RemoveTool,
    StepName::Wipe,
    StepName::Record,
];
const FORGET: [StepName; 1] = [StepName::Record];

impl Jobs {
    /// Starts decommissioning `node` (D-131), `firstboot` resetting it to its image's defaults
    /// too, and returns the job as it starts.
    pub async fn decommission(
        &self,
        node: NodeId,
        firstboot: bool,
        by: Option<Author>,
    ) -> Result<Job> {
        if self.shared.dial.is_none() {
            return Err(Error::Unavailable);
        }
        let node = self.leaving(node).await?;
        let job = job(&node, JobKind::Decommission, by.as_ref());
        self.spawn(keys(&node), job, move |shared, progress| {
            run(shared, node, Some(firstboot), by, progress)
        })
        .await
    }

    /// Starts forgetting `node` on the controller alone (D-131), and returns the job as it
    /// starts.
    pub async fn forget(&self, node: NodeId, by: Option<Author>) -> Result<Job> {
        let node = self.leaving(node).await?;
        let job = job(&node, JobKind::Forget, by.as_ref());
        self.spawn(keys(&node), job, move |shared, progress| {
            run(shared, node, None, by, progress)
        })
        .await
    }

    /// The node about to leave the site, unless a rule of the site keeps it.
    async fn leaving(&self, id: NodeId) -> Result<Node> {
        let store = &self.shared.store;
        let nodes = store.nodes().await?;
        let node = nodes
            .iter()
            .find(|node| node.id == id)
            .ok_or(Error::NoNode(id))?
            .clone();
        if node.role == NodeRole::Gateway && nodes.len() > 1 {
            return Err(Error::Refused(format!(
                "{} is the site's gateway and the other devices depend on it: remove them \
                 first, or replace it",
                node.name
            )));
        }
        if let Some(rollout) = store.active_rollout().await?
            && rollout.nodes.iter().any(|entry| {
                entry.node == id
                    && matches!(
                        entry.state,
                        RolloutNodeState::Pending
                            | RolloutNodeState::Preparing
                            | RolloutNodeState::Upgrading
                    )
            })
        {
            return Err(Error::Refused(format!(
                "{} is still to be upgraded by rollout {}: skip it there, or cancel the rollout",
                node.name, rollout.id
            )));
        }
        Ok(node)
    }
}

fn keys(node: &Node) -> Vec<String> {
    vec![manage::target(node.id), apply::TARGET.to_owned()]
}

fn job(node: &Node, kind: JobKind, by: Option<&Author>) -> NewJob {
    NewJob {
        kind,
        target: node.name.clone(),
        node: Some(node.id),
        revision: None,
        created_by: by.map(|author| author.id),
        steps: if kind == JobKind::Forget {
            FORGET.to_vec()
        } else {
            DECOMMISSION.to_vec()
        },
        nodes: Vec::new(),
    }
}

/// Decommissions `node` (`firstboot` given) or forgets it (`None`).
async fn run(
    shared: Arc<Shared>,
    node: Node,
    firstboot: Option<bool>,
    by: Option<Author>,
    mut progress: Progress,
) {
    let result = match firstboot {
        Some(firstboot) => wipe(&shared, &node, firstboot, &mut progress).await,
        None => Ok(()),
    };
    let result = match result {
        Ok(()) => {
            let forgot = forget::forget(&shared, &node, by.as_ref(), &mut progress).await;
            if forgot.is_err() {
                watch_again(&shared, &node).await;
            }
            forgot
        }
        Err(err) => Err(err),
    };
    match &result {
        Ok(()) => info!(node = %node.id, wiped = firstboot.is_some(), "the node left the site"),
        Err(err) => warn!(node = %node.id, code = ?err.code, "{}", err.message),
    }
    progress.finish(result.err()).await;
}

/// The steps on the node, up to the wipe started.
async fn wipe(
    shared: &Shared,
    node: &Node,
    firstboot: bool,
    progress: &mut Progress,
) -> Result<(), JobError> {
    let remote = manage::connect(shared, node, progress).await?;
    let result = on_node(shared, node, remote.as_ref(), firstboot, progress).await;
    remote.close().await;
    if result.is_err() {
        watch_again(shared, node).await;
    }
    result
}

async fn on_node(
    shared: &Shared,
    node: &Node,
    remote: &dyn Remote,
    firstboot: bool,
    progress: &mut Progress,
) -> Result<(), JobError> {
    manage::remove(remote, progress).await?;
    if node.mode == NodeMode::Managed {
        // The tool is gone: an apply must not count on it, whatever happens next.
        shared
            .store
            .set_node_mode(node.id, NodeMode::Observe)
            .await
            .map_err(|err| internal("recording the node's mode", &err))?;
    }

    progress.start(StepName::Wipe).await;
    shared.watch.unwatch(node.id).await;
    let key = shared
        .key
        .authorized_key()
        .map_err(|err| internal("cannot encode the controller's key", &err))?;
    let argv = decommission::wipe(&key, firstboot);
    let output = remote
        .exec(uniwrt_node::ExecRequest::new(
            argv.iter().map(String::as_str),
        ))
        .await
        .and_then(|output| output.check("sh"))
        .map_err(|err| node_error(&err, "Cannot start the wipe"))?;
    if !decommission::started(&String::from_utf8_lossy(&output.stdout)) {
        return Err(JobError {
            code: JobErrorCode::NodeFailed,
            message: "The wipe did not start on the node".to_owned(),
        });
    }
    progress
        .done(Some(
            if firstboot {
                "Started on the device: uniwrt's files and key go, then it resets to its \
                 defaults (firstboot) and restarts"
            } else {
                "Started on the device: it leaves the mesh, uniwrt's files and key go; its \
                 other settings stay"
            }
            .to_owned(),
        ))
        .await;
    Ok(())
}

/// Watches the node again after a decommission that stopped before its wipe, as it was.
async fn watch_again(shared: &Shared, node: &Node) {
    let pin = match shared.store.host_key(node.id).await {
        Ok(Some(pin)) => pin,
        Ok(None) => return,
        Err(err) => {
            warn!(%err, node = %node.id, "cannot read the host key to watch the node again");
            return;
        }
    };
    let (Ok(host_key), Ok(Some(node))) =
        (HostKey::from_pin(&pin), shared.store.node(node.id).await)
    else {
        warn!(node = %node.id, "cannot watch the node again");
        return;
    };
    shared.watch.moved(node, host_key).await;
}
