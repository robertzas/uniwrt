//! Drift (features/lifecycle.md, Drift; D-072, D-074):
//!
//! - `GET /api/v1/drift`: every drift (a managed package that changed on its node after the
//!   controller confirmed it, with its diff, secrets masked) and every muted package
//!   ([`DriftState`]); live over the WS topic `config`.
//! - `POST /api/v1/nodes/{id}/drift/{package}/acknowledge` (admins): accepts the drift as it is
//!   until the file changes again; 204, or 404 without such a drift. Audited.
//! - `PUT /api/v1/nodes/{id}/drift/{package}/mute` with `{"muted": true|false}` (admins): stops
//!   watching the node's package, or watches it again; 204. Audited.
//!
//! Importing a drift is `POST /api/v1/import/preview` with `{"drift": {"node", "package"}}`;
//! overwriting it is an apply, reviewed on `/changes`.

use axum::{
    Json,
    extract::{
        Path, State,
        rejection::{JsonRejection, PathRejection},
    },
    http::StatusCode,
};
use serde::Deserialize;
use ts_rs::TS;
use uniwrt_core::NodeId;
use uniwrt_engine::DriftError;
use uniwrt_model::{drift::DriftState, user::AuditAction};

use crate::{
    Api, ApiError, ApiResult, Client, CurrentUser,
    alerts::{admin, audit},
};

#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct DriftMuteBody {
    /// Stop watching (`true`) or watch again.
    pub muted: bool,
}

pub(crate) async fn get(State(api): State<Api>, _user: CurrentUser) -> ApiResult<Json<DriftState>> {
    Ok(Json((*api.drift()?.state()).clone()))
}

pub(crate) async fn acknowledge(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    path: Result<Path<(i64, String)>, PathRejection>,
) -> ApiResult<StatusCode> {
    admin(&user, "acknowledge drift")?;
    let (node, package) = target(path)?;
    api.drift()?
        .acknowledge(node, package.clone(), Some(user.user.username.clone()))
        .await
        .map_err(drift_error)?;
    let detail = format!("acknowledged the drift of {package} on node {node}");
    audit(&api, &user, &client, AuditAction::Drift, detail).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn mute(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    path: Result<Path<(i64, String)>, PathRejection>,
    body: Result<Json<DriftMuteBody>, JsonRejection>,
) -> ApiResult<StatusCode> {
    admin(&user, "mute drift")?;
    let (node, package) = target(path)?;
    let Json(DriftMuteBody { muted }) = body?;
    api.drift()?
        .mute(
            node,
            package.clone(),
            muted,
            Some(user.user.username.clone()),
        )
        .await
        .map_err(drift_error)?;
    let detail = if muted {
        format!("muted the drift of {package} on node {node}")
    } else {
        format!("watches the drift of {package} on node {node} again")
    };
    audit(&api, &user, &client, AuditAction::Drift, detail).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The node and package of a path, the package named as UCI names packages.
fn target(path: Result<Path<(i64, String)>, PathRejection>) -> ApiResult<(NodeId, String)> {
    let Path((node, package)) =
        path.map_err(|_| ApiError::BadRequest("A node ID is a number".to_owned()))?;
    check_package(&package)?;
    Ok((NodeId::new(node), package))
}

pub(crate) fn check_package(package: &str) -> ApiResult<()> {
    let valid = !package.is_empty()
        && package.len() <= 64
        && package
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if valid {
        Ok(())
    } else {
        Err(ApiError::BadRequest(format!(
            "{package:?} is not a UCI package name"
        )))
    }
}

pub(crate) fn drift_error(err: DriftError) -> ApiError {
    match err {
        DriftError::NotFound { .. } => ApiError::NotFound(err.to_string()),
        DriftError::Stopped => ApiError::Unavailable("Drift detection is not running".to_owned()),
        DriftError::Store(err) => err.into(),
    }
}

#[cfg(test)]
mod tests;
