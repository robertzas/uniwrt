//! Firmware upgrades (features/lifecycle.md, M7.1a1; D-121):
//!
//! - `GET /api/v1/firmware` ([`FirmwareCatalog`]): every adopted node's release and the
//!   upgrades on offer (the latest of its branch, the next branch's), each with the device's
//!   profile in that release, or why none can be offered, and the uploaded images that fit it.
//!   ASU off or unreachable gives an `error` instead of offers (uploads are still listed).
//! - `POST /api/v1/firmware/uploads?name=<file name>` and `DELETE …/uploads/{sha256}`:
//!   `firmware_uploads.rs`.
//! - `POST /api/v1/firmware/nodes/{id}/plan` ([`PlanBody`]; [`UpgradePlan`]), for admins: with
//!   ASU or an official image (D-122), reads the node's packages again (observe mode too:
//!   nothing changes on it) and works out what the upgrade keeps, renames, adds, drops and
//!   removes, baked into ASU's image or installed after the official one (whose files it lists);
//!   with an uploaded image, checks that it fits the node's board and reads nothing.
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
use uniwrt_firmware::{Error, Upgrade, upload_fits};
use uniwrt_model::{
    facts::NodeFacts,
    firmware::{
        FirmwareCatalog, FirmwareUpload, ImageBuild, ImageBuildStarted, ImageSource, NodeFirmware,
        UpgradePlan,
    },
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
        .merge(crate::firmware_uploads::routes())
}

/// The image to plan a node's upgrade with (D-122).
#[derive(Debug, Deserialize, TS)]
#[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
#[ts(export)]
pub enum PlanBody {
    /// Built by ASU: `release` is one of the node's offers.
    Asu { release: String },
    /// OpenWrt's image of `release`, one of the node's offers.
    Official { release: String },
    /// The uploaded image `sha256`.
    Upload { sha256: String },
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
    let uploads = match api.uploads() {
        Ok(uploads) => uploads.list().await.map_err(firmware_error)?,
        Err(_) => Vec::new(),
    };
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
        entry.uploads = fitting(&uploads, &facts);
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
        uploads,
        error,
    }))
}

/// The uploads that fit a node, by SHA-256 (newest first, as listed).
fn fitting(uploads: &[FirmwareUpload], facts: &NodeFacts) -> Vec<String> {
    uploads
        .iter()
        .filter(|upload| upload_fits(upload, &facts.board.release.target, &facts.board.board_name))
        .map(|upload| upload.sha256.clone())
        .collect()
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
        uploads: Vec::new(),
    }
}

pub(crate) async fn plan(
    State(api): State<Api>,
    user: CurrentUser,
    id: Result<Path<i64>, PathRejection>,
    body: Result<Json<PlanBody>, JsonRejection>,
) -> ApiResult<Json<UpgradePlan>> {
    admin(&user.user)?;
    let Json(body) = body?;
    let (release, official) = match body {
        PlanBody::Upload { sha256 } => return upload_plan(&api, id, &sha256).await.map(Json),
        PlanBody::Asu { release } => (release, false),
        PlanBody::Official { release } => (release, true),
    };
    let (node, facts, inventory) = read(&api, id, &release).await?;
    let asu = api.asu()?;
    let upgrade = upgrade(node.id, &facts, &inventory, &release);
    let plan = if official {
        asu.official_plan(upgrade).await
    } else {
        asu.upgrade_plan(upgrade).await
    };
    Ok(Json(plan.map_err(firmware_error)?))
}

/// An uploaded image's plan: it must fit the node; it is flashed as it is, so nothing is read.
async fn upload_plan(
    api: &Api,
    id: Result<Path<i64>, PathRejection>,
    sha256: &str,
) -> ApiResult<UpgradePlan> {
    let (node, facts) = node_facts(api, id).await?;
    let upload = api
        .uploads()?
        .get(sha256)
        .await
        .map_err(firmware_error)?
        .ok_or_else(|| ApiError::NotFound(format!("There is no uploaded image {sha256}")))?;
    let board = &facts.board;
    if !upload_fits(&upload, &board.release.target, &board.board_name) {
        return Err(ApiError::Conflict(format!(
            "{} is made for {} ({}), not {} ({})",
            upload.name,
            if upload.boards.is_empty() {
                "any board".to_owned()
            } else {
                upload.boards.join(", ")
            },
            upload.target,
            node.name,
            board.board_name,
        )));
    }
    Ok(UpgradePlan {
        node: node.id,
        source: ImageSource::Upload,
        from: board.release.version.clone(),
        release: upload
            .version
            .clone()
            .unwrap_or_else(|| upload.name.clone()),
        target: upload.target.clone(),
        profile: String::new(),
        packages: Vec::new(),
        kept: Vec::new(),
        renamed: Vec::new(),
        added: Vec::new(),
        dropped: Vec::new(),
        removed: Vec::new(),
        packages_at: None,
        image_bytes: Some(upload.size_bytes),
        tmp_free_bytes: facts.system.tmp.map(|tmp| tmp.available_bytes),
        images: Vec::new(),
        upload: Some(upload),
    })
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

pub(crate) fn admin(user: &User) -> ApiResult<()> {
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
    let (node, facts) = node_facts(api, id).await?;
    let id = node.id;
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

/// The node and its facts.
async fn node_facts(
    api: &Api,
    id: Result<Path<i64>, PathRejection>,
) -> ApiResult<(Node, std::sync::Arc<NodeFacts>)> {
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
    Ok((node, facts))
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
        Error::Storage(_) => ApiError::Internal(err.to_string()),
    }
}
