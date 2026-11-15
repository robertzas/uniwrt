//! Firmware upgrades (features/lifecycle.md, M7.1a1; D-121):
//!
//! - `GET /api/v1/firmware` ([`FirmwareCatalog`]): every adopted node's release and the
//!   upgrades on offer (the latest of its branch, the next branch's), each with the device's
//!   profile in that release, or why none can be offered. ASU off or unreachable gives an
//!   `error` instead of offers.
//! - `POST /api/v1/firmware/nodes/{id}/plan` ([`UpgradeBody`]; [`UpgradePlan`]), for admins:
//!   reads the node's packages again (observe mode too: nothing changes on it) and works out
//!   what an image of the release keeps, renames, adds, drops and removes.
//! - `POST /api/v1/firmware/nodes/{id}/builds` ([`UpgradeBody`]; [`ImageBuildStarted`]), for
//!   admins: reads the packages again and asks ASU for that image (no first-boot script: the
//!   upgrade keeps the configuration). Audited (`firmware`).
//! - `GET /api/v1/firmware/builds/{hash}` ([`ImageBuild`]): how a build stands.

use std::collections::BTreeMap;

use axum::{
    Json,
    extract::{
        Path, State,
        rejection::{JsonRejection, PathRejection},
    },
};
use serde::Deserialize;
use ts_rs::TS;
use uniwrt_core::NodeId;
use uniwrt_firmware::{Error, Upgrade};
use uniwrt_model::{
    facts::NodeFacts,
    firmware::{FirmwareCatalog, ImageBuild, ImageBuildStarted, NodeFirmware, UpgradePlan},
    inventory::Inventory,
    node::Node,
    user::{AuditAction, AuditEntry, Role, User},
};
use uniwrt_store::{AuditRepo as _, NodeRepo as _};

use crate::{Api, ApiError, ApiResult, Client, CurrentUser, jobs::jobs_error};

pub(crate) fn routes() -> axum::Router<Api> {
    use axum::routing::{get, post};
    axum::Router::new()
        .route("/v1/firmware", get(catalog))
        .route("/v1/firmware/nodes/{id}/plan", post(plan))
        .route("/v1/firmware/nodes/{id}/builds", post(build))
        .route("/v1/firmware/builds/{hash}", get(status))
}

/// The release to upgrade a node to.
#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct UpgradeBody {
    /// One of the node's offers in `GET v1/firmware`.
    pub release: String,
}

pub(crate) async fn catalog(
    State(api): State<Api>,
    _: CurrentUser,
) -> ApiResult<Json<FirmwareCatalog>> {
    let views: BTreeMap<_, _> = api.fleet().all().into_iter().collect();
    let nodes = api.store().nodes().await?;
    let asu = api.asu().map_err(|_| {
        "Firmware upgrades are off: the controller runs without ASU (--asu-url off)".to_owned()
    });
    let mut error = asu.as_ref().err().cloned();
    let mut listed = Vec::with_capacity(nodes.len());
    for node in &nodes {
        let facts = views.get(&node.id).and_then(|view| view.facts.clone());
        let Some(facts) = facts else {
            listed.push(NodeFirmware {
                note: Some("Waiting for the node's facts".to_owned()),
                ..firmware(node.id, &NodeFacts::default())
            });
            continue;
        };
        let mut entry = firmware(node.id, &facts);
        if let (Ok(asu), None) = (&asu, &error) {
            match asu
                .offers(&facts.board.release, &facts.board.board_name)
                .await
            {
                Ok((offers, note)) => {
                    entry.offers = offers;
                    entry.note = note;
                }
                // One failure stands for all: ASU is down or its answers are off.
                Err(err) => error = Some(firmware_error(err).to_string()),
            }
        }
        listed.push(entry);
    }
    Ok(Json(FirmwareCatalog {
        nodes: listed,
        error,
    }))
}

fn firmware(node: NodeId, facts: &NodeFacts) -> NodeFirmware {
    let release = &facts.board.release;
    NodeFirmware {
        node,
        distribution: release.distribution.clone(),
        version: release.version.clone(),
        revision: release.revision.clone(),
        target: release.target.clone(),
        board: facts.board.board_name.clone(),
        offers: Vec::new(),
        note: None,
    }
}

pub(crate) async fn plan(
    State(api): State<Api>,
    user: CurrentUser,
    id: Result<Path<i64>, PathRejection>,
    body: Result<Json<UpgradeBody>, JsonRejection>,
) -> ApiResult<Json<UpgradePlan>> {
    admin(&user.user)?;
    let Json(body) = body?;
    let (node, facts, inventory) = read(&api, id, &body.release).await?;
    let plan = api
        .asu()?
        .upgrade_plan(upgrade(node.id, &facts, &inventory, &body.release))
        .await
        .map_err(firmware_error)?;
    Ok(Json(plan))
}

pub(crate) async fn build(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    id: Result<Path<i64>, PathRejection>,
    body: Result<Json<UpgradeBody>, JsonRejection>,
) -> ApiResult<Json<ImageBuildStarted>> {
    let user = user.user;
    admin(&user)?;
    let Json(body) = body?;
    let (node, facts, inventory) = read(&api, id, &body.release).await?;
    let started = api
        .asu()?
        .build_upgrade(upgrade(node.id, &facts, &inventory, &body.release))
        .await
        .map_err(firmware_error)?;
    api.store()
        .record(AuditEntry {
            at: api.now(),
            action: AuditAction::Firmware,
            user_id: Some(user.id),
            username: Some(user.username.clone()),
            ip: Some(client.ip.to_string()),
            detail: Some(format!(
                "asked ASU for an upgrade image of {} (node {}) from {} to {} ({})",
                node.name, node.id, facts.board.release.version, body.release, started.build.hash
            )),
        })
        .await?;
    Ok(Json(started))
}

pub(crate) async fn status(
    State(api): State<Api>,
    _: CurrentUser,
    hash: Result<Path<String>, PathRejection>,
) -> ApiResult<Json<ImageBuild>> {
    let Path(hash) = hash.map_err(|_| ApiError::BadRequest("A build is needed".to_owned()))?;
    Ok(Json(
        api.asu()?.status(&hash).await.map_err(firmware_error)?,
    ))
}

fn admin(user: &User) -> ApiResult<()> {
    if user.role == Role::Admin {
        Ok(())
    } else {
        Err(ApiError::Forbidden(
            "Only admins prepare firmware upgrades".to_owned(),
        ))
    }
}

/// The node, its facts and its packages read again, once `release` is known to be on offer for
/// it (so nothing is read for a release it can't get).
async fn read(
    api: &Api,
    id: Result<Path<i64>, PathRejection>,
    release: &str,
) -> ApiResult<(Node, std::sync::Arc<NodeFacts>, Inventory)> {
    let Path(id) = id.map_err(|_| ApiError::BadRequest("A node ID is a number".to_owned()))?;
    let id = NodeId::new(id);
    let node = api
        .store()
        .node(id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("There is no node {id}")))?;
    let facts = api
        .fleet()
        .node(id)
        .and_then(|view| view.facts)
        .ok_or_else(|| ApiError::Conflict("The node's facts haven't arrived yet".to_owned()))?;
    let (offers, why) = api
        .asu()?
        .offers(&facts.board.release, &facts.board.board_name)
        .await
        .map_err(firmware_error)?;
    if !offers
        .iter()
        .any(|offer| offer.release == release && offer.profile.is_some())
    {
        return Err(ApiError::NotFound(why.unwrap_or_else(|| {
            format!(
                "{release} isn't an upgrade on offer for {} on {}",
                node.name, facts.board.release.version
            )
        })));
    }
    let manager = facts.capabilities.package_manager.ok_or_else(|| {
        ApiError::Conflict("The node has no package manager uniwrt knows".to_owned())
    })?;
    let inventory = api
        .jobs()
        .read_packages(id, manager)
        .await
        .map_err(|err| match err {
            uniwrt_jobs::Error::Failed(why) => ApiError::BadGateway(why),
            other => jobs_error(other),
        })?;
    Ok((node, facts, inventory))
}

fn upgrade<'a>(
    node: NodeId,
    facts: &'a NodeFacts,
    inventory: &'a Inventory,
    to: &'a str,
) -> Upgrade<'a> {
    Upgrade {
        node,
        release: &facts.board.release,
        board: &facts.board.board_name,
        to,
        packages: &inventory.packages,
        packages_at: inventory.taken_at,
        tmp_free_bytes: facts.system.tmp.map(|tmp| tmp.available_bytes),
    }
}

/// What the firmware client's errors mean to a client.
pub(crate) fn firmware_error(err: Error) -> ApiError {
    match err {
        Error::NotFound(why) => ApiError::NotFound(why),
        Error::Busy(_) => ApiError::Unavailable(err.to_string()),
        Error::Unreachable { .. }
        | Error::Status { .. }
        | Error::Invalid(_)
        | Error::Refused(_) => ApiError::BadGateway(err.to_string()),
    }
}
