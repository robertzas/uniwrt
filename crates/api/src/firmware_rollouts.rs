//! Rolling upgrades (M7.1c2, D-125): `GET /api/v1/firmware/rollouts` (the latest 20, newest
//! first) and `…/rollouts/{id}`; for admins, `POST …/rollouts` ([`RolloutBody`]: each device's
//! image as the upgrade dialog chose it, the canary, the window; 202 with the rollout), `POST
//! …/rollouts/{id}/pause`, `…/resume` (a halted rollout tries its failed device again),
//! `…/cancel` and `…/rollouts/{id}/nodes/{node}/skip`, each answering the rollout as it is
//! then. The driver (`uniwrt_jobs::Rollouts`) upgrades the devices one at a time, leaves first
//! and the gateway last, each by its own upgrade job (`GET v1/jobs/{id}`, the WS hub's `jobs`
//! topic). Audited (`firmware`).

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
use uniwrt_core::{NodeId, RolloutId};
use uniwrt_jobs::{CanaryRequest, RolloutRequest};
use uniwrt_model::{
    rollout::{Rollout, UpgradeChoice, UpgradeWindow},
    user::{AuditAction, AuditEntry, User},
};
use uniwrt_store::AuditRepo as _;

use crate::{Api, ApiError, ApiResult, Client, CurrentUser, firmware::admin, jobs::jobs_error};

/// How many rollouts the list holds.
const LISTED: usize = 20;

pub(crate) fn routes() -> axum::Router<Api> {
    use axum::routing::{get, post};
    axum::Router::new()
        .route("/v1/firmware/rollouts", get(list).post(create))
        .route("/v1/firmware/rollouts/{id}", get(one))
        .route("/v1/firmware/rollouts/{id}/pause", post(pause))
        .route("/v1/firmware/rollouts/{id}/resume", post(resume))
        .route("/v1/firmware/rollouts/{id}/cancel", post(cancel))
        .route("/v1/firmware/rollouts/{id}/nodes/{node}/skip", post(skip))
}

/// A rolling upgrade to start.
#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct RolloutBody {
    /// Each device and its image, in any order: the controller orders them.
    pub nodes: Vec<RolloutNodeBody>,
    /// Soak one device first; ignored for a single device.
    #[serde(default)]
    pub canary: Option<CanaryBody>,
    /// Start devices only within it, daily in the site's time; at any time without.
    #[serde(default)]
    pub window: Option<UpgradeWindow>,
}

#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct RolloutNodeBody {
    pub node: NodeId,
    /// One of its offers (or a fitting upload), as its plan was shown.
    pub choice: UpgradeChoice,
}

#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct CanaryBody {
    /// The canary; the first device in order without.
    #[serde(default)]
    pub node: Option<NodeId>,
    /// Minutes, 30 to 1440.
    pub soak_min: u32,
}

async fn list(State(api): State<Api>, _: CurrentUser) -> ApiResult<Json<Vec<Rollout>>> {
    Ok(Json(api.rollouts().list(LISTED).await.map_err(error)?))
}

async fn one(
    State(api): State<Api>,
    _: CurrentUser,
    id: Result<Path<i64>, PathRejection>,
) -> ApiResult<Json<Rollout>> {
    let id = rollout_id(id)?;
    let rollout = api.rollouts().get(id).await.map_err(error)?;
    Ok(Json(rollout.ok_or_else(|| {
        ApiError::NotFound(format!("There is no rollout {id}"))
    })?))
}

async fn create(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    body: Result<Json<RolloutBody>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<Rollout>)> {
    let user = user.user;
    admin(&user)?;
    let Json(body) = body?;
    api.images()?;
    let request = RolloutRequest {
        nodes: body
            .nodes
            .into_iter()
            .map(|node| (node.node, node.choice))
            .collect(),
        canary: body.canary.map(|canary| CanaryRequest {
            node: canary.node,
            soak_min: canary.soak_min,
        }),
        window: body.window,
        facts: api
            .fleet()
            .all()
            .into_iter()
            .filter_map(|(id, view)| Some((id, view.facts?)))
            .collect(),
    };
    let rollout = api
        .rollouts()
        .create(request, Some(user.id))
        .await
        .map_err(error)?;
    let names: Vec<&str> = rollout
        .nodes
        .iter()
        .map(|node| node.name.as_str())
        .collect();
    let detail = format!(
        "started rollout {} upgrading {} in this order{}{}",
        rollout.id,
        names.join(", "),
        rollout
            .canary
            .map_or_default(|canary| format!(", the first soaking {} min", canary.soak_min)),
        rollout
            .window
            .map_or_default(|window| format!(", within {}", span(window)))
    );
    audit(&api, &user, &client, detail).await?;
    Ok((StatusCode::ACCEPTED, Json(rollout)))
}

async fn pause(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    id: Result<Path<i64>, PathRejection>,
) -> ApiResult<Json<Rollout>> {
    let user = user.user;
    admin(&user)?;
    let rollout = api.rollouts().pause(rollout_id(id)?).await.map_err(error)?;
    audit(
        &api,
        &user,
        &client,
        format!("paused rollout {}", rollout.id),
    )
    .await?;
    Ok(Json(rollout))
}

async fn resume(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    id: Result<Path<i64>, PathRejection>,
) -> ApiResult<Json<Rollout>> {
    let user = user.user;
    admin(&user)?;
    let rollout = api
        .rollouts()
        .resume(rollout_id(id)?)
        .await
        .map_err(error)?;
    audit(
        &api,
        &user,
        &client,
        format!("resumed rollout {}", rollout.id),
    )
    .await?;
    Ok(Json(rollout))
}

async fn cancel(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    id: Result<Path<i64>, PathRejection>,
) -> ApiResult<Json<Rollout>> {
    let user = user.user;
    admin(&user)?;
    let rollout = api
        .rollouts()
        .cancel(rollout_id(id)?)
        .await
        .map_err(error)?;
    audit(
        &api,
        &user,
        &client,
        format!("cancelled rollout {}", rollout.id),
    )
    .await?;
    Ok(Json(rollout))
}

async fn skip(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    ids: Result<Path<(i64, i64)>, PathRejection>,
) -> ApiResult<Json<Rollout>> {
    let user = user.user;
    admin(&user)?;
    let Path((id, node)) =
        ids.map_err(|_| ApiError::BadRequest("A rollout and a node ID are numbers".to_owned()))?;
    let (id, node) = (RolloutId::new(id), NodeId::new(node));
    let rollout = api.rollouts().skip(id, node).await.map_err(error)?;
    let name = rollout
        .nodes
        .iter()
        .find(|entry| entry.node == node)
        .map_or("?", |entry| entry.name.as_str());
    let detail = format!("skipped {name} (node {node}) in rollout {id}");
    audit(&api, &user, &client, detail).await?;
    Ok(Json(rollout))
}

fn rollout_id(id: Result<Path<i64>, PathRejection>) -> ApiResult<RolloutId> {
    let Path(id) = id.map_err(|_| ApiError::BadRequest("A rollout ID is a number".to_owned()))?;
    Ok(RolloutId::new(id))
}

fn error(err: uniwrt_jobs::Error) -> ApiError {
    use uniwrt_jobs::Error;
    match err {
        Error::NoRollout(id) => ApiError::NotFound(format!("There is no rollout {id}")),
        Error::Observed(id) => ApiError::Conflict(format!(
            "Node {id} is in observe mode: turn its management on before upgrading it"
        )),
        other => jobs_error(other),
    }
}

/// `03:30–05:30`.
fn span(window: UpgradeWindow) -> String {
    let at = |min: u16| format!("{:02}:{:02}", min / 60, min % 60);
    format!("{}–{}", at(window.start_min), at(window.end_min))
}

async fn audit(api: &Api, user: &User, client: &Client, detail: String) -> ApiResult<()> {
    api.store()
        .record(AuditEntry {
            at: api.now(),
            action: AuditAction::Firmware,
            user_id: Some(user.id),
            username: Some(user.username.clone()),
            ip: Some(client.ip.to_string()),
            detail: Some(detail),
        })
        .await?;
    Ok(())
}
