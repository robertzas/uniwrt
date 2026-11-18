//! The apply's Backup step (D-126): each node the apply changes is backed up over its login
//! before anything changes, a few at a time. A node that can't be backed up stops the apply
//! with nothing changed.

use futures_util::{StreamExt as _, stream};
use uniwrt_model::{
    inventory::BackupReason,
    job::{JobError, JobErrorCode, JobNodeState, StepName},
};

use super::run::Live;
use crate::{backup::take, progress::Progress, runner::Shared};

/// How many nodes are backed up at once.
const AT_ONCE: usize = 4;

pub(super) async fn back_up(
    shared: &Shared,
    lives: &[Live],
    progress: &mut Progress,
) -> Result<(), JobError> {
    progress.start(StepName::Backup).await;
    let backups: Vec<_> = lives
        .iter()
        .map(|live| async move {
            let kept = match &live.remote {
                Some(remote) => take(shared, live.node.id, remote.as_ref(), BackupReason::Apply)
                    .await
                    .map_err(|err| err.message),
                None => Err("its login was lost".to_owned()),
            };
            (live, kept)
        })
        .collect();
    let mut ends = stream::iter(backups).buffer_unordered(AT_ONCE);
    let mut failed = Vec::new();
    while let Some((live, kept)) = ends.next().await {
        if let Err(why) = kept {
            progress
                .node(live.node.id, JobNodeState::Failed, &why)
                .await;
            failed.push(format!("{}: {why}", live.node.name));
        }
    }
    if !failed.is_empty() {
        return Err(JobError {
            code: JobErrorCode::NodeFailed,
            message: format!(
                "Nothing changed: a node to change couldn't be backed up. {}",
                failed.join(". ")
            ),
        });
    }
    let n = lives.len();
    progress
        .done(Some(if n == 1 {
            "The node to change is backed up".to_owned()
        } else {
            format!("The {n} nodes to change are backed up")
        }))
        .await;
    Ok(())
}
