//! Configuration backups of nodes (features/lifecycle.md, Backups; D-126). The backup job logs
//! in to each node (observe mode too: `sysupgrade -b` changes nothing), takes its backup and
//! keeps it, sealed ([`take`]); a few nodes at a time, each reported as it ends, and a node that
//! fails doesn't stop the others. Weekly, or by an admin (*Back up now*). [`restore`] puts a
//! backup's configuration back onto its node.

pub(crate) mod restore;
mod take;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use futures_util::{StreamExt as _, stream};
use tracing::{info, warn};
use uniwrt_core::UserId;
use uniwrt_model::{
    inventory::BackupReason,
    job::{JobError, JobErrorCode, JobKind, JobNode, JobNodeState, NewJob, StepName},
    node::Node,
};

pub(crate) use self::take::{content, read, take};
use crate::{manage::login, progress::Progress, runner::Shared};

/// How many nodes are backed up at once.
const AT_ONCE: usize = 4;

pub(crate) fn job(nodes: &[Node], by: Option<UserId>) -> NewJob {
    let names: Vec<&str> = nodes.iter().map(|node| node.name.as_str()).collect();
    NewJob {
        kind: JobKind::Backup,
        target: names.join(", "),
        node: (nodes.len() == 1).then(|| nodes[0].id),
        revision: None,
        created_by: by,
        steps: vec![StepName::Backup],
        nodes: nodes
            .iter()
            .map(|node| JobNode {
                node: node.id,
                name: node.name.clone(),
                state: JobNodeState::Pending,
                detail: None,
            })
            .collect(),
    }
}

pub(crate) async fn run(
    shared: Arc<Shared>,
    nodes: Vec<Node>,
    reason: BackupReason,
    mut progress: Progress,
) {
    let result = back_up(&shared, &nodes, reason, &mut progress).await;
    match &result {
        Ok(()) => info!(nodes = nodes.len(), ?reason, "nodes backed up"),
        Err(err) => warn!(code = ?err.code, "{}", err.message),
    }
    progress.finish(result.err()).await;
}

async fn back_up(
    shared: &Shared,
    nodes: &[Node],
    reason: BackupReason,
    progress: &mut Progress,
) -> Result<(), JobError> {
    progress.start(StepName::Backup).await;
    let backups: Vec<_> = nodes
        .iter()
        .map(|node| async move {
            let kept = match login(shared, node).await {
                Ok(remote) => {
                    let kept = take(shared, node.id, remote.as_ref(), reason).await;
                    remote.close().await;
                    kept
                }
                Err(err) => Err(err),
            };
            (node, kept)
        })
        .collect();
    let mut ends = stream::iter(backups).buffer_unordered(AT_ONCE);
    let (mut kept, mut failed) = (0, Vec::new());
    while let Some((node, result)) = ends.next().await {
        match result {
            Ok(detail) => {
                kept += 1;
                progress.node(node.id, JobNodeState::BackedUp, detail).await;
            }
            Err(err) => {
                progress
                    .node(node.id, JobNodeState::Failed, &err.message)
                    .await;
                failed.push(format!("{}: {}", node.name, err.message));
            }
        }
    }
    if !failed.is_empty() {
        let mut message = failed.join(". ");
        if kept > 0 {
            message = format!("{message}. The other {} backed up", count(kept));
        }
        return Err(JobError {
            code: JobErrorCode::NodeFailed,
            message,
        });
    }
    progress
        .done(Some(format!("Backed up {}", count(kept))))
        .await;
    Ok(())
}

fn count(n: usize) -> String {
    if n == 1 {
        "1 node".to_owned()
    } else {
        format!("{n} nodes")
    }
}
