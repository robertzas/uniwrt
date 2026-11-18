//! The controller's side of a node leaving the site (D-131): it stops watching the node, a
//! revision leaves it out of the site's document ([`uniwrt_model::site::remove`]), and its
//! record goes with everything stored about it ([`uniwrt_store::NodeRepo::delete_node`]).

use std::fmt::Write as _;

use uniwrt_core::RevisionId;
use uniwrt_model::{
    job::{JobError, StepName},
    node::Node,
    site::{remove::remove_node, revision::RevisionKind},
};
use uniwrt_store::NodeRepo as _;

use crate::{
    errors::internal,
    progress::Progress,
    revise::{Author, revise},
    runner::Shared,
};

pub(super) async fn forget(
    shared: &Shared,
    node: &Node,
    by: Option<&Author>,
    progress: &mut Progress,
) -> Result<(), JobError> {
    progress.start(StepName::Record).await;
    shared.watch.unwatch(node.id).await;
    let revision = leave_out(shared, node, by).await?;
    shared
        .store
        .delete_node(node.id)
        .await
        .map_err(|err| internal("removing the node", &err))?;
    let mut detail = "Removed from the controller with its history".to_owned();
    if let Some(revision) = revision {
        let _ = write!(
            detail,
            "; revision {revision} leaves its settings out (nothing to apply for the others)"
        );
    }
    progress.done(Some(detail)).await;
    Ok(())
}

/// Commits the revision without the node, if the latest names it.
async fn leave_out(
    shared: &Shared,
    node: &Node,
    by: Option<&Author>,
) -> Result<Option<RevisionId>, JobError> {
    let message = format!("Removed {}", node.name);
    revise(shared, by, RevisionKind::Remove, &message, |config| {
        remove_node(config, node.id, &[node.mac])
    })
    .await
}
