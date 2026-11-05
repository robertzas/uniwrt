//! Brownfield import, with an admin's session (features/lifecycle.md, Brownfield import;
//! D-072, D-073):
//!
//! - `POST /api/v1/import/preview`: reads every adopted node (observe mode too; nothing changes
//!   on them) and answers what importing their configuration into the latest revision would
//!   change ([`ImportPreview`]): the site's changes, and per node its confidence, sections, raw
//!   sections and residual, secrets masked. The controller holds the newest preview, its
//!   secrets sealed, for [`HELD`]; a new preview replaces it.
//! - `POST /api/v1/import/accept` with `{"id": …, "message": …}` ([`ImportAccept`]): saves the
//!   held preview as the next revision (kind `import`), as reviewed: 404 when it is not the
//!   held one (or expired), 409 once another revision was saved since, 422 while its document
//!   has errors. A preview that changes nothing saves nothing. Audited.

use std::{fmt::Write as _, time::Duration};

use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
};
use ring::rand::{SecureRandom as _, SystemRandom};
use serde::Deserialize;
use ts_rs::TS;
use uniwrt_core::Timestamp;
use uniwrt_jobs::ImportDraft;
use uniwrt_model::{
    import::ImportPreview,
    site::{
        revision::{NewRevision, RevisionKind},
        validate::{Severity, has_errors},
    },
    user::AuditAction,
};
use uniwrt_store::RevisionRepo as _;

use crate::{
    Api, ApiError, ApiResult, Client, CurrentUser,
    alerts::{admin, audit},
    jobs::jobs_error,
    site::{SiteView, ensure_first_revision, message, view},
};

/// How long a preview can be accepted.
pub(crate) const HELD: Duration = Duration::from_mins(15);

#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct ImportAccept {
    /// The preview reviewed ([`ImportPreview::id`]).
    pub id: String,
    pub message: Option<String>,
}

/// The newest preview, as the controller holds it until accepted, replaced or expired.
pub(crate) struct Held {
    at: Timestamp,
    draft: ImportDraft,
}

pub(crate) async fn preview(
    State(api): State<Api>,
    user: CurrentUser,
) -> ApiResult<Json<ImportPreview>> {
    admin(&user, "import the nodes' configuration")?;
    ensure_first_revision(&api, None).await?;
    let mut draft = api.jobs().import().await.map_err(import_error)?;
    draft.preview.id = random_id()?;
    let preview = draft.preview.clone();
    *api.imports() = Some(Held {
        at: api.now(),
        draft,
    });
    Ok(Json(preview))
}

pub(crate) async fn accept(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    body: Result<Json<ImportAccept>, JsonRejection>,
) -> ApiResult<Json<SiteView>> {
    admin(&user, "import the nodes' configuration")?;
    let Json(ImportAccept { id, message: text }) = body?;
    let message = match message(text)? {
        text if text.is_empty() => "Imported from the nodes".to_owned(),
        text => text,
    };
    let draft = take(&api, &id)?;
    if has_errors(&draft.preview.issues) {
        let errors = draft
            .preview
            .issues
            .iter()
            .filter(|issue| issue.severity == Severity::Error)
            .count();
        let issues = draft.preview.issues.clone();
        // Kept: the user can still read it, though it can't be accepted as it is.
        *api.imports() = Some(Held {
            at: api.now(),
            draft,
        });
        let detail = if errors == 1 {
            "The import has an error".to_owned()
        } else {
            format!("The import has {errors} errors")
        };
        return Err(ApiError::Invalid(detail, issues));
    }
    let head = api
        .store()
        .head_revision()
        .await?
        .ok_or_else(|| ApiError::Unavailable("The site has no configuration yet".to_owned()))?;
    if head.meta.id != draft.preview.base {
        return Err(ApiError::Conflict(format!(
            "Revision {} was saved since this preview: import again",
            head.meta.id
        )));
    }
    if !draft.preview.changes() && draft.sealed.is_empty() {
        return Ok(Json(view(&api, head.meta, head.config).await?));
    }
    let new = NewRevision {
        base: Some(draft.preview.base),
        kind: RevisionKind::Import,
        restored_from: None,
        author_id: Some(user.user.id),
        author: Some(user.user.username.clone()),
        message,
        config: draft.site.clone(),
    };
    let meta = api.store().commit_revision(new, draft.sealed).await?;
    api.config_changed();
    let detail = format!(
        "imported revision {} from the nodes ({} changes)",
        meta.id, meta.changes
    );
    audit(&api, &user, &client, AuditAction::Site, detail).await?;
    Ok(Json(view(&api, meta, draft.site).await?))
}

/// Takes the held preview named `id`, if it hasn't expired.
fn take(api: &Api, id: &str) -> ApiResult<ImportDraft> {
    let mut held = api.imports();
    let fresh = held.as_ref().is_some_and(|held| {
        held.draft.preview.id == id && api.now().saturating_duration_since(held.at) <= HELD
    });
    match held.take() {
        Some(held) if fresh => Ok(held.draft),
        other => {
            *held = other;
            Err(ApiError::NotFound(
                "This preview is no longer held: import again".to_owned(),
            ))
        }
    }
}

fn import_error(err: uniwrt_jobs::Error) -> ApiError {
    match err {
        uniwrt_jobs::Error::NoRevision => {
            ApiError::Conflict("The site has no revision yet".to_owned())
        }
        uniwrt_jobs::Error::Busy { .. } => {
            ApiError::Conflict("An apply is running: import once it has finished".to_owned())
        }
        err => jobs_error(err),
    }
}

fn random_id() -> ApiResult<String> {
    let mut id = [0_u8; 16];
    SystemRandom::new()
        .fill(&mut id)
        .map_err(|_| ApiError::Internal("the random number generator failed".to_owned()))?;
    Ok(id.iter().fold(String::with_capacity(32), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    }))
}

#[cfg(test)]
mod tests;
