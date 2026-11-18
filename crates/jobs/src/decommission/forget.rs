//! The controller's side of a node leaving the site (D-131): it stops watching the node, a
//! revision leaves it out of the site's document ([`uniwrt_model::site::remove`]), and its
//! record goes with everything stored about it ([`uniwrt_store::NodeRepo::delete_node`]).

use std::fmt::Write as _;

use uniwrt_core::RevisionId;
use uniwrt_model::{
    job::{JobError, StepName},
    node::Node,
    site::{
        remove::remove_node,
        revision::{NewRevision, RevisionKind},
    },
};
use uniwrt_store::{NodeRepo as _, RevisionRepo as _};

use super::Author;
use crate::{errors::internal, progress::Progress, runner::Shared};

/// How often a revision saved meanwhile is read again before giving up.
const TRIES: usize = 3;

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
    for _ in 0..TRIES {
        let Some(head) = shared
            .store
            .head_revision()
            .await
            .map_err(|err| internal("reading the site's configuration", &err))?
        else {
            return Ok(None);
        };
        let mut config = head.config;
        if !remove_node(&mut config, node.id, &[node.mac]) {
            return Ok(None);
        }
        let new = NewRevision {
            base: Some(head.meta.id),
            kind: RevisionKind::Remove,
            restored_from: None,
            author_id: by.map(|author| author.id),
            author: by.map(|author| author.name.clone()),
            message: format!("Removed {}", node.name),
            config,
        };
        match shared.store.commit_revision(new, Vec::new()).await {
            Ok(meta) => return Ok(Some(meta.id)),
            Err(uniwrt_store::Error::NotLatest { .. }) => {}
            Err(err) => return Err(internal("saving the site without the node", &err)),
        }
    }
    Err(internal(
        "saving the site without the node",
        &"other revisions kept being saved meanwhile",
    ))
}
