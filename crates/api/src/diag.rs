//! The diagnostics toolkit's runs (features/observability.md, Diagnostics toolkit; D-141), with
//! a session: `GET v1/diag/runs` lists the runs kept, newest first, without their lines;
//! `GET v1/diag/runs/{id}` is one with its lines, which the WS hub's `diag:<id>` topic
//! follows. Admins: `POST v1/diag/runs` ([`DiagRunBody`]) starts a tool on an adopted node
//! (202 with the run; audited; 409 while too many run; an iperf3 test's peer node must be
//! adopted too, D-142), and `DELETE v1/diag/runs/{id}` stops
//! one (the run as it is; it ends shortly). A node's tables and optional tools are in
//! [`tables`], a packet capture's download in [`capture`].

mod capture;
mod tables;

pub(crate) use tables::node;

use axum::{
    Json,
    extract::{Path, State, rejection::JsonRejection},
    http::{StatusCode, header},
    response::{IntoResponse as _, Response},
    routing::get,
};
use serde::Deserialize;
use ts_rs::TS;
use uniwrt_core::NodeId;
use uniwrt_engine::{DiagError, RunHosts};
use uniwrt_model::{
    diag::{DiagRun, DiagTool, iperf3::Iperf3Peer},
    user::{AuditAction, AuditEntry, Role},
};
use uniwrt_store::{AuditRepo as _, NodeRepo as _};

use crate::{Api, ApiError, ApiResult, Client, CurrentUser};

/// What `POST v1/diag/runs` asks: a tool to run on a node.
#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct DiagRunBody {
    pub node: NodeId,
    pub tool: DiagTool,
}

pub(crate) fn routes() -> axum::Router<Api> {
    axum::Router::new()
        .route("/v1/diag/runs", get(list).post(start))
        .route("/v1/diag/runs/{id}", get(one).delete(stop))
        .merge(tables::routes())
        .merge(capture::routes())
}

async fn list(State(api): State<Api>, _user: CurrentUser) -> Json<Vec<DiagRun>> {
    Json(api.diag().list())
}

async fn one(
    State(api): State<Api>,
    _user: CurrentUser,
    Path(id): Path<u32>,
) -> ApiResult<Json<DiagRun>> {
    api.diag().get(id).map(Json).ok_or_else(|| not_found(id))
}

async fn start(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    body: Result<Json<DiagRunBody>, JsonRejection>,
) -> ApiResult<Response> {
    let Json(body) = body?;
    let user = admin(user, "Only admins run diagnostics on devices")?;
    let node = api
        .store()
        .node(body.node)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("There is no node {}", body.node)))?;
    let mut hosts = RunHosts {
        node: Some(node.host.clone()),
        peer: None,
    };
    let target = match &body.tool {
        DiagTool::Iperf3 {
            peer: Iperf3Peer::Node(peer),
            ..
        } => {
            let peer = api.store().node(*peer).await?.ok_or_else(|| {
                ApiError::BadRequest(format!("There is no node {peer} to test against"))
            })?;
            hosts.peer = Some(peer.host);
            format!("with {}", peer.name)
        }
        DiagTool::Iperf3 { .. } => "with the controller".to_owned(),
        other => other.target().into_owned(),
    };
    let detail = format!("{} {target} on {}", body.tool.name(), node.name);
    let run = api
        .diag()
        .start(node.id, body.tool, Some(user.username.clone()), hosts)
        .map_err(diag_error)?;
    api.store()
        .record(AuditEntry {
            at: api.now(),
            action: AuditAction::Diagnostics,
            user_id: Some(user.id),
            username: Some(user.username),
            ip: Some(client.ip.to_string()),
            detail: Some(format!("{detail}, run {}", run.id)),
        })
        .await?;
    let location = format!("/api/v1/diag/runs/{}", run.id);
    Ok((
        StatusCode::ACCEPTED,
        [(header::LOCATION, location)],
        Json(run),
    )
        .into_response())
}

async fn stop(
    State(api): State<Api>,
    user: CurrentUser,
    Path(id): Path<u32>,
) -> ApiResult<Json<DiagRun>> {
    admin(user, "Only admins stop diagnostics")?;
    api.diag().stop(id).map(Json).map_err(|err| match err {
        DiagError::NotFound => not_found(id),
        other => diag_error(other),
    })
}

fn admin(user: CurrentUser, refusal: &str) -> ApiResult<uniwrt_model::user::User> {
    if user.user.role == Role::Admin {
        Ok(user.user)
    } else {
        Err(ApiError::Forbidden(refusal.to_owned()))
    }
}

fn not_found(id: u32) -> ApiError {
    ApiError::NotFound(format!(
        "There is no run {id}: runs are kept for 15 minutes after they end, and not across restarts"
    ))
}

fn diag_error(err: DiagError) -> ApiError {
    match err {
        DiagError::Invalid(why) => ApiError::BadRequest(why),
        DiagError::Busy(why) => ApiError::Conflict(why),
        DiagError::NotFound => ApiError::NotFound("There is no such run".to_owned()),
    }
}

#[cfg(test)]
mod tests;
