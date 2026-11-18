//! Node backups (features/lifecycle.md, Backups; D-126). With a session: `GET
//! v1/nodes/{id}/backups` (the node's [`Backup`]s, newest first). Admins: `POST
//! v1/nodes/{id}/backups` (*Back up now*: a job, 202), `GET v1/nodes/{id}/backups/{backup}/archive`
//! (the `sysupgrade -b` tarball, which holds the node's password hashes, host keys and Wi-Fi
//! keys: audited), `POST v1/nodes/{id}/backups/{backup}/restore` (its `/etc/config` packages
//! back onto the managed node as a transaction: a job, 202; 409 in observe mode). Every admin
//! action is audited.

use axum::{
    Json,
    extract::{Path, State, rejection::PathRejection},
    http::{StatusCode, header},
    response::{IntoResponse as _, Response},
};
use uniwrt_core::{BackupId, NodeId};
use uniwrt_model::{
    inventory::{Backup, BackupReason},
    job::Job,
    user::{AuditAction, AuditEntry, Role, User},
};
use uniwrt_store::{AuditRepo as _, BackupRepo as _, NodeRepo as _};

use crate::{Api, ApiError, ApiResult, Client, CurrentUser, jobs::jobs_error};

pub(crate) fn routes() -> axum::Router<Api> {
    use axum::routing::{get, post};
    axum::Router::new()
        .route("/v1/nodes/{id}/backups", get(list).post(back_up))
        .route("/v1/nodes/{id}/backups/{backup}/archive", get(archive))
        .route("/v1/nodes/{id}/backups/{backup}/restore", post(restore))
}

fn node_id(id: Result<Path<i64>, PathRejection>) -> Result<NodeId, ApiError> {
    let Path(id) = id.map_err(|_| ApiError::BadRequest("A node ID is a number".to_owned()))?;
    Ok(NodeId::new(id))
}

fn path_ids(ids: Result<Path<(i64, i64)>, PathRejection>) -> Result<(NodeId, BackupId), ApiError> {
    let Path((node, backup)) =
        ids.map_err(|_| ApiError::BadRequest("Node and backup IDs are numbers".to_owned()))?;
    Ok((NodeId::new(node), BackupId::new(backup)))
}

fn admin(user: &User, what: &str) -> Result<(), ApiError> {
    if user.role == Role::Admin {
        Ok(())
    } else {
        Err(ApiError::Forbidden(format!("Only admins {what}")))
    }
}

async fn node_name(api: &Api, id: NodeId) -> Result<String, ApiError> {
    Ok(api
        .store()
        .node(id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("There is no node {id}")))?
        .name)
}

async fn audit(api: &Api, user: &User, client: &Client, detail: String) -> ApiResult<()> {
    api.store()
        .record(AuditEntry {
            at: api.now(),
            action: AuditAction::Backup,
            user_id: Some(user.id),
            username: Some(user.username.clone()),
            ip: Some(client.ip.to_string()),
            detail: Some(detail),
        })
        .await?;
    Ok(())
}

fn accepted(job: Job) -> Response {
    let location = format!("/api/v1/jobs/{}", job.id);
    (
        StatusCode::ACCEPTED,
        [(header::LOCATION, location)],
        Json(job),
    )
        .into_response()
}

pub(crate) async fn list(
    State(api): State<Api>,
    _user: CurrentUser,
    id: Result<Path<i64>, PathRejection>,
) -> ApiResult<Json<Vec<Backup>>> {
    let id = node_id(id)?;
    node_name(&api, id).await?;
    Ok(Json(api.store().backups(id).await?))
}

pub(crate) async fn back_up(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    id: Result<Path<i64>, PathRejection>,
) -> ApiResult<Response> {
    let id = node_id(id)?;
    let user = user.user;
    admin(&user, "back up devices")?;
    let job = api
        .jobs()
        .back_up(&[id], BackupReason::Manual, Some(user.id))
        .await
        .map_err(jobs_error)?;
    let detail = format!("backed up {} (node {id}), job {}", job.target, job.id);
    audit(&api, &user, &client, detail).await?;
    Ok(accepted(job))
}

pub(crate) async fn archive(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    ids: Result<Path<(i64, i64)>, PathRejection>,
) -> ApiResult<Response> {
    let (node, backup) = path_ids(ids)?;
    let user = user.user;
    admin(
        &user,
        "download backups: they hold the device's passwords and keys",
    )?;
    let name = node_name(&api, node).await?;
    let (found, tarball) = api
        .vault()?
        .open_backup(backup)
        .await?
        .filter(|(found, _)| found.node == node)
        .ok_or_else(|| ApiError::NotFound(format!("Node {node} has no backup {backup}")))?;
    let file = file_name(&name, &found);
    audit(
        &api,
        &user,
        &client,
        format!("downloaded the backup {backup} of {name} (node {node}) as {file}"),
    )
    .await?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/gzip".to_owned()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{file}\""),
            ),
            (header::CACHE_CONTROL, "no-store".to_owned()),
        ],
        tarball.to_vec(),
    )
        .into_response())
}

pub(crate) async fn restore(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    ids: Result<Path<(i64, i64)>, PathRejection>,
) -> ApiResult<Response> {
    let (node, backup) = path_ids(ids)?;
    let user = user.user;
    admin(&user, "restore backups")?;
    let job = api
        .jobs()
        .restore(node, backup, Some(user.id))
        .await
        .map_err(|err| match err {
            uniwrt_jobs::Error::Observed(_) => ApiError::Conflict(
                "The device is in observe mode: turn its management on first".to_owned(),
            ),
            uniwrt_jobs::Error::NoBackup(node, backup) => {
                ApiError::NotFound(format!("Node {node} has no backup {backup}"))
            }
            other => jobs_error(other),
        })?;
    let detail = format!(
        "restored backup {backup} onto {} (node {node}), job {}",
        job.target, job.id
    );
    audit(&api, &user, &client, detail).await?;
    Ok(accepted(job))
}

/// `<node>-<YYYYMMDD>-<HHMM>.tar.gz`, the node's name kept to safe characters.
fn file_name(node: &str, backup: &Backup) -> String {
    let name: String = node
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let digits: String = backup
        .taken_at
        .to_string()
        .chars()
        .filter(char::is_ascii_digit)
        .take(12)
        .collect();
    let (date, time) = digits.split_at(digits.len().min(8));
    format!("{name}-{date}-{time}.tar.gz")
}

#[cfg(test)]
#[path = "backups_tests.rs"]
mod tests;
