//! Restarting nodes (features/network.md, Other services; D-137). Admins: `POST v1/reboots`
//! ([`RebootBody`]) restarts the managed nodes named, one at a time, the gateway last (a job,
//! 202; 409 when one is in observe mode); audited.

use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
    http::{StatusCode, header},
    response::{IntoResponse as _, Response},
};
use serde::Deserialize;
use ts_rs::TS;
use uniwrt_core::NodeId;
use uniwrt_model::user::{AuditAction, AuditEntry, Role};
use uniwrt_store::AuditRepo as _;

use crate::{Api, ApiError, ApiResult, Client, CurrentUser, jobs::jobs_error};

/// What `POST v1/reboots` asks.
#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct RebootBody {
    /// The nodes to restart, in any order: the job orders them.
    pub nodes: Vec<NodeId>,
}

pub(crate) fn routes() -> axum::Router<Api> {
    axum::Router::new().route("/v1/reboots", axum::routing::post(post))
}

async fn post(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    body: Result<Json<RebootBody>, JsonRejection>,
) -> ApiResult<Response> {
    let Json(body) = body?;
    let user = user.user;
    if user.role != Role::Admin {
        return Err(ApiError::Forbidden(
            "Only admins restart devices".to_owned(),
        ));
    }
    if body.nodes.is_empty() {
        return Err(ApiError::BadRequest(
            "Name the devices to restart".to_owned(),
        ));
    }
    let job = api
        .jobs()
        .reboot(&body.nodes, Some(user.id))
        .await
        .map_err(|err| match err {
            uniwrt_jobs::Error::Observed(id) => ApiError::Conflict(format!(
                "Node {id} is in observe mode: turn its management on first"
            )),
            other => jobs_error(other),
        })?;
    let names: Vec<String> = job.nodes.iter().map(|node| node.name.clone()).collect();
    api.store()
        .record(AuditEntry {
            at: api.now(),
            action: AuditAction::Reboot,
            user_id: Some(user.id),
            username: Some(user.username.clone()),
            ip: Some(client.ip.to_string()),
            detail: Some(format!("restarted {}, job {}", names.join(", "), job.id)),
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
