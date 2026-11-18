//! Revisions a job commits for an admin (D-131): the site's latest document edited and saved
//! on top of it, read again when another save came first.

use uniwrt_core::{RevisionId, UserId};
use uniwrt_model::{
    job::JobError,
    site::{
        SiteConfig,
        revision::{NewRevision, RevisionKind},
    },
};
use uniwrt_store::RevisionRepo as _;

use crate::{errors::internal, runner::Shared};

/// Who asked, for the revision a job commits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Author {
    pub id: UserId,
    pub name: String,
}

/// How often a revision saved meanwhile is read again before giving up.
const TRIES: usize = 3;

/// Commits `edit` of the latest revision, when there is one and `edit` changes it (true).
pub(crate) async fn revise(
    shared: &Shared,
    by: Option<&Author>,
    kind: RevisionKind,
    message: &str,
    mut edit: impl FnMut(&mut SiteConfig) -> bool + Send,
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
        if !edit(&mut config) {
            return Ok(None);
        }
        let new = NewRevision {
            base: Some(head.meta.id),
            kind,
            restored_from: None,
            author_id: by.map(|author| author.id),
            author: by.map(|author| author.name.clone()),
            message: message.to_owned(),
            config,
        };
        match shared.store.commit_revision(new, Vec::new()).await {
            Ok(meta) => return Ok(Some(meta.id)),
            Err(uniwrt_store::Error::NotLatest { .. }) => {}
            Err(err) => return Err(internal("saving the site's configuration", &err)),
        }
    }
    Err(internal(
        "saving the site's configuration",
        &"other revisions kept being saved meanwhile",
    ))
}
