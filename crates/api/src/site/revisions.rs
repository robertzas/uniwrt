//! The site's history, with a session (features/lifecycle.md, Desired state & revisions):
//!
//! - `GET /api/v1/revisions?before=<id>&limit=<n>`: pages of revisions, newest first
//!   ([`RevisionPage`]; 50 by default, at most 200).
//! - `GET /api/v1/revisions/{id}`: one with its document and its changes from its parent.
//! - `GET /api/v1/revisions/{id}/diff?against=<id>`: the changes from another revision (its
//!   parent by default) to this one ([`RevisionDiff`]).
//! - `POST /api/v1/revisions/{id}/restore` (admins; audited): its document becomes the next
//!   revision ([`RestoreBody`]: `base` must still be the latest).

use axum::{
    Json,
    extract::{Path, Query, State, rejection::JsonRejection},
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uniwrt_core::RevisionId;
use uniwrt_model::{
    site::{
        diff::{Change, diff},
        revision::{NewRevision, Revision, RevisionKind, RevisionMeta},
    },
    user::AuditAction,
};
use uniwrt_store::RevisionRepo as _;

use super::{SiteView, checked, message, view};
use crate::{
    Api, ApiError, ApiResult, Client, CurrentUser,
    alerts::{admin, audit},
};

const DEFAULT_PAGE: usize = 50;
const MAX_PAGE: usize = 200;

#[derive(Debug, Deserialize)]
pub(crate) struct PageQuery {
    before: Option<RevisionId>,
    limit: Option<usize>,
}

/// Revisions, newest first.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RevisionPage {
    pub revisions: Vec<RevisionMeta>,
    /// Whether older ones follow: ask with `before` set to the last one's id.
    pub more: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DiffQuery {
    against: Option<RevisionId>,
}

/// The changes from one revision to another.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RevisionDiff {
    /// `None` when `to` is the first revision and nothing was named to compare with.
    pub from: Option<RevisionId>,
    pub to: RevisionId,
    pub changes: Vec<Change>,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct RestoreBody {
    /// The latest revision as the user saw it.
    pub base: RevisionId,
    #[serde(default)]
    #[ts(optional)]
    pub message: Option<String>,
}

pub(crate) async fn list(
    State(api): State<Api>,
    _: CurrentUser,
    Query(query): Query<PageQuery>,
) -> ApiResult<Json<RevisionPage>> {
    let limit = query.limit.unwrap_or(DEFAULT_PAGE).clamp(1, MAX_PAGE);
    let mut revisions = api.store().revisions(query.before, limit + 1).await?;
    let more = revisions.len() > limit;
    revisions.truncate(limit);
    Ok(Json(RevisionPage { revisions, more }))
}

pub(crate) async fn get(
    State(api): State<Api>,
    _: CurrentUser,
    Path(id): Path<RevisionId>,
) -> ApiResult<Json<Revision>> {
    Ok(Json(revision(&api, id).await?))
}

pub(crate) async fn changes(
    State(api): State<Api>,
    _: CurrentUser,
    Path(id): Path<RevisionId>,
    Query(query): Query<DiffQuery>,
) -> ApiResult<Json<RevisionDiff>> {
    let to = revision(&api, id).await?;
    let Some(from) = query.against.or(to.meta.parent) else {
        return Ok(Json(RevisionDiff {
            from: None,
            to: id,
            changes: Vec::new(),
        }));
    };
    let changes = if Some(from) == to.meta.parent {
        to.diff
    } else {
        diff(&revision(&api, from).await?.config, &to.config)
    };
    Ok(Json(RevisionDiff {
        from: Some(from),
        to: id,
        changes,
    }))
}

pub(crate) async fn restore(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    Path(id): Path<RevisionId>,
    body: Result<Json<RestoreBody>, JsonRejection>,
) -> ApiResult<Json<SiteView>> {
    admin(&user, "restore revisions")?;
    let Json(body) = body?;
    let message = message(body.message)?;
    let old = revision(&api, id).await?;
    let head = api
        .store()
        .head_revision()
        .await?
        .ok_or_else(super::no_site)?;
    if old.config == head.config {
        return Ok(Json(view(&api, head.meta, head.config).await?));
    }
    let issues = checked(&api, &old.config).await?;
    let new = NewRevision {
        base: Some(body.base),
        kind: RevisionKind::Restore,
        restored_from: Some(id),
        author_id: Some(user.user.id),
        author: Some(user.user.username.clone()),
        message: if message.is_empty() {
            format!("Restored revision {id}")
        } else {
            message
        },
        config: old.config.clone(),
    };
    let meta = api.store().commit_revision(new, Vec::new()).await?;
    let detail = format!("restored revision {id} as revision {}", meta.id);
    audit(&api, &user, &client, AuditAction::Site, detail).await?;
    Ok(Json(SiteView {
        secrets: super::secret_states(&old.config),
        revision: meta,
        config: old.config,
        issues,
    }))
}

async fn revision(api: &Api, id: RevisionId) -> ApiResult<Revision> {
    api.store().revision(id).await?.ok_or_else(|| missing(id))
}

pub(super) fn missing(id: RevisionId) -> ApiError {
    ApiError::NotFound(format!("There is no revision {id}"))
}
