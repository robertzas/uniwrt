//! The per-node package manager (features/network.md, Other services; D-139).
//! `GET v1/nodes/{id}/packages` ([`NodePackages`]): the installed packages as last read, each
//! with its lock. Admins: `POST v1/nodes/{id}/packages/catalog` ([`PackageCatalog`]) reads what
//! the managed node's feeds offer (its index refreshed); `POST v1/nodes/{id}/packages`
//! ([`PackageChanges`]) starts the job that changes them (202; 409 in observe mode; 422 for a
//! malformed or locked name); audited.

use axum::{
    Json,
    extract::{Path, State, rejection::JsonRejection, rejection::PathRejection},
    http::{StatusCode, header},
    response::{IntoResponse as _, Response},
    routing::{get, post},
};
use uniwrt_core::NodeId;
use uniwrt_model::{
    packages::{NodePackages, PackageCatalog, PackageChanges, PackageLock},
    user::{AuditAction, AuditEntry, Role},
};
use uniwrt_store::AuditRepo as _;

use crate::{Api, ApiError, ApiResult, Client, CurrentUser, jobs::jobs_error};

pub(crate) fn routes() -> axum::Router<Api> {
    axum::Router::new()
        .route("/v1/nodes/{id}/packages", get(list).post(change))
        .route("/v1/nodes/{id}/packages/catalog", post(catalog))
}

fn node_id(id: Result<Path<i64>, PathRejection>) -> Result<NodeId, ApiError> {
    let Path(id) = id.map_err(|_| ApiError::BadRequest("A node ID is a number".to_owned()))?;
    Ok(NodeId::new(id))
}

fn admin(user: &CurrentUser) -> Result<(), ApiError> {
    if user.user.role == Role::Admin {
        Ok(())
    } else {
        Err(ApiError::Forbidden(
            "Only admins change the packages of devices".to_owned(),
        ))
    }
}

fn error(err: uniwrt_jobs::Error) -> ApiError {
    match err {
        uniwrt_jobs::Error::Observed(_) => ApiError::Conflict(
            "The device is in observe mode: turn its management on first".to_owned(),
        ),
        uniwrt_jobs::Error::Refused(why) => ApiError::Invalid(why, Vec::new()),
        uniwrt_jobs::Error::Failed(why) => ApiError::BadGateway(why),
        other => jobs_error(other),
    }
}

async fn list(
    State(api): State<Api>,
    _user: CurrentUser,
    id: Result<Path<i64>, PathRejection>,
) -> ApiResult<Json<NodePackages>> {
    let id = node_id(id)?;
    Ok(Json(
        api.jobs().installed_packages(id).await.map_err(error)?,
    ))
}

async fn catalog(
    State(api): State<Api>,
    user: CurrentUser,
    id: Result<Path<i64>, PathRejection>,
) -> ApiResult<Json<PackageCatalog>> {
    let id = node_id(id)?;
    admin(&user)?;
    Ok(Json(api.jobs().package_catalog(id).await.map_err(error)?))
}

async fn change(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    id: Result<Path<i64>, PathRejection>,
    body: Result<Json<PackageChanges>, JsonRejection>,
) -> ApiResult<Response> {
    let id = node_id(id)?;
    let Json(changes) = body?;
    admin(&user)?;
    // The locks as last known, so that a locked name is refused at once; the job checks again.
    let listed = api.jobs().installed_packages(id).await.map_err(error)?;
    for name in &changes.remove {
        let lock = listed
            .packages
            .iter()
            .find(|package| &package.name == name)
            .and_then(|package| package.lock);
        let why = match lock {
            Some(PackageLock::Required) => "the site's configuration needs it",
            Some(PackageLock::Essential) => "the device needs it",
            None => continue,
        };
        return Err(ApiError::Invalid(
            format!("{name} is locked: {why}"),
            Vec::new(),
        ));
    }
    let user = user.user;
    let job = api
        .jobs()
        .packages(id, changes.clone(), Some(user.id))
        .await
        .map_err(error)?;
    let mut said = Vec::new();
    for (verb, names) in [
        ("install", &changes.install),
        ("remove", &changes.remove),
        ("upgrade", &changes.upgrade),
    ] {
        if !names.is_empty() {
            said.push(format!("{verb} {}", names.join(", ")));
        }
    }
    api.store()
        .record(AuditEntry {
            at: api.now(),
            action: AuditAction::Packages,
            user_id: Some(user.id),
            username: Some(user.username.clone()),
            ip: Some(client.ip.to_string()),
            detail: Some(format!(
                "changed the packages of {} (node {id}): {}, job {}",
                job.target,
                said.join("; "),
                job.id
            )),
        })
        .await?;
    let location = format!("/api/v1/jobs/{}", job.id);
    Ok((
        StatusCode::ACCEPTED,
        [(header::LOCATION, location)],
        Json(job),
    )
        .into_response())
}

#[cfg(test)]
mod tests;
