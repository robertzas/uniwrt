//! Installing a prepared upgrade (M7.1c1, D-124): `POST /api/v1/firmware/nodes/{id}/install`
//! ([`UpgradeInstallBody`]), for admins, starts the upgrade job on a managed node and answers 202 with
//! it (`GET /api/v1/jobs/{id}` and the WS hub's `jobs` topic follow it); 409 for a node in
//! observe mode. The image is resolved here from the source the plan named, never from a URL
//! the client sends: ASU's finished build of an offered release, OpenWrt's official image of an
//! offer (its packages installed after, D-122), an uploaded image that fits the node, or a
//! release on offer from the node's GitHub repo. Images are downloaded into the controller's
//! cache ([`ImageCache`]) and checked against their SHA-256 as the job's Download step; an
//! official image the feed mirror keeps (D-147) is copied from there instead. Audited
//! (`firmware`).

use std::{path::PathBuf, sync::Arc};

use axum::{
    Json,
    extract::{
        Path, State,
        rejection::{JsonRejection, PathRejection},
    },
    http::{StatusCode, header},
    response::{IntoResponse as _, Response},
};
use serde::Deserialize;
use ts_rs::TS;
use uniwrt_engine::FeedMirror;
use uniwrt_firmware::{GitHub, ImageCache};
use uniwrt_jobs::{BoxFuture, FetchImage, UpgradeRequest};
use uniwrt_model::{
    facts::NodeFacts,
    firmware::{FirmwareImage, ImageBuildState},
    node::{Node, NodeMode},
    user::{AuditAction, AuditEntry},
};
use uniwrt_store::AuditRepo as _;
use zeroize::Zeroizing;

use crate::{
    Api, ApiError, ApiResult, Client, CurrentUser,
    firmware::{admin, firmware_error, node_facts, read, upgrade, upload_plan},
    firmware_sources,
    jobs::jobs_error,
};

/// The prepared upgrade to install (the plan's source, D-122).
#[derive(Debug, Deserialize, TS)]
#[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
#[ts(export)]
pub enum UpgradeInstallBody {
    /// ASU's build `build` (its hash) of `release`, one of the node's offers; it must be done.
    Asu { release: String, build: String },
    /// OpenWrt's image of `release`, one of the node's offers.
    Official { release: String },
    /// The uploaded image `sha256`.
    Upload { sha256: String },
    /// The release tagged `release` of the node's GitHub repo, one on offer for it.
    Github { release: String },
}

pub(crate) async fn install(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    id: Result<Path<i64>, PathRejection>,
    body: Result<Json<UpgradeInstallBody>, JsonRejection>,
) -> ApiResult<Response> {
    let user = user.user;
    admin(&user)?;
    let Json(body) = body?;
    let (node, facts) = node_facts(&api, id).await?;
    if node.mode != NodeMode::Managed {
        return Err(observed());
    }
    let (request, what) = upgrade_request(&api, &node, &facts, body).await?;
    let (from, to) = (request.from.clone(), request.to.clone());
    let job = api
        .jobs()
        .upgrade(request, Some(user.id))
        .await
        .map_err(|err| match err {
            uniwrt_jobs::Error::Observed(_) => observed(),
            other => jobs_error(other),
        })?;
    api.store()
        .record(AuditEntry {
            at: api.now(),
            action: AuditAction::Firmware,
            user_id: Some(user.id),
            username: Some(user.username.clone()),
            ip: Some(client.ip.to_string()),
            detail: Some(format!(
                "started upgrading {} (node {}) from {from} to {} with {what}, job {}",
                node.name,
                node.id,
                to.as_deref().unwrap_or("another build"),
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

/// The upgrade job's request for `body`'s image on `node`, and the image's source for people
/// (the install's, and a rollout's at each node's turn).
pub(crate) async fn upgrade_request(
    api: &Api,
    node: &Node,
    facts: &NodeFacts,
    body: UpgradeInstallBody,
) -> ApiResult<(UpgradeRequest, String)> {
    let Resolved {
        what,
        to,
        images,
        packages,
        fetch,
    } = resolve(api, node, facts, body).await?;
    if images.is_empty() {
        return Err(ApiError::NotFound(format!(
            "{what} has no image for {}",
            node.name
        )));
    }
    let request = UpgradeRequest {
        node: node.id,
        from: facts.board.release.version.clone(),
        to,
        images,
        packages,
        fetch,
    };
    Ok((request, what))
}

/// What an install flashes, resolved from its source.
struct Resolved {
    /// The image's source, for the audit.
    what: String,
    to: Option<String>,
    images: Vec<FirmwareImage>,
    packages: Vec<String>,
    fetch: Arc<dyn FetchImage>,
}

/// The images of `body`'s source for `node`, checked as the plan checks them.
async fn resolve(
    api: &Api,
    node: &Node,
    facts: &NodeFacts,
    body: UpgradeInstallBody,
) -> ApiResult<Resolved> {
    let cache = api.images()?.clone();
    let (what, to, images, packages, fetch): (String, _, _, _, Arc<dyn FetchImage>) = match body {
        UpgradeInstallBody::Asu { release, build } => {
            let images = asu_images(api, facts, &release, &build).await?;
            (
                format!("ASU's build {build}"),
                Some(release),
                images,
                Vec::new(),
                Arc::new(Fetch::Cache(cache, None)),
            )
        }
        UpgradeInstallBody::Official { release } => {
            let (node, facts, inventory) = read(api, Ok(Path(node.id.get())), &release).await?;
            let plan = api
                .asu()?
                .official_plan(upgrade(node.id, &facts, &inventory, &release))
                .await
                .map_err(firmware_error)?;
            (
                "OpenWrt's image".to_owned(),
                Some(release),
                plan.images,
                plan.packages,
                Arc::new(Fetch::Cache(cache, api.feeds().ok().cloned())),
            )
        }
        UpgradeInstallBody::Upload { sha256 } => {
            let plan = upload_plan(api, Ok(Path(node.id.get())), &sha256).await?;
            let upload = plan.upload.ok_or_else(|| {
                ApiError::Internal("an upload's plan without the upload".to_owned())
            })?;
            let path = api.uploads()?.path(&sha256).ok_or_else(|| {
                ApiError::NotFound(format!("There is no uploaded image {sha256}"))
            })?;
            let image = FirmwareImage {
                kind: kind_of(&upload.name).to_owned(),
                name: upload.name.clone(),
                filesystem: None,
                size_bytes: Some(upload.size_bytes),
                sha256: upload.sha256.clone(),
                url: String::new(),
            };
            (
                format!("the uploaded {}", upload.name),
                upload.version,
                vec![image],
                Vec::new(),
                Arc::new(Fetch::Upload(path)),
            )
        }
        UpgradeInstallBody::Github { release } => {
            let plan = crate::firmware_github::plan(api, node, facts, &release).await?;
            let repo = plan.repo.unwrap_or_default();
            let fetch = Fetch::GitHub {
                github: api.github()?.clone(),
                cache,
                repo: repo.clone(),
                tag: release.clone(),
                token: firmware_sources::token(api).await?,
            };
            (
                format!("{repo}'s release {release}"),
                None,
                plan.images,
                Vec::new(),
                Arc::new(fetch),
            )
        }
    };
    Ok(Resolved {
        what,
        to,
        images,
        packages,
        fetch,
    })
}

fn observed() -> ApiError {
    ApiError::Conflict(
        "The device is in observe mode: turn its management on before upgrading it".to_owned(),
    )
}

/// ASU's build `hash` of `release` for the node: `release` on offer, the build done and of
/// that release.
async fn asu_images(
    api: &Api,
    facts: &NodeFacts,
    release: &str,
    hash: &str,
) -> ApiResult<Vec<FirmwareImage>> {
    let asu = api.asu()?;
    let (offers, why) = asu
        .offers(&facts.board.release, &facts.board.board_name)
        .await
        .map_err(firmware_error)?;
    if !offers.iter().any(|offer| offer.release == release) {
        return Err(ApiError::NotFound(why.unwrap_or_else(|| {
            format!("{release} isn't an upgrade on offer for the device")
        })));
    }
    let build = asu.status(hash).await.map_err(firmware_error)?;
    if build.state != ImageBuildState::Done {
        return Err(ApiError::Conflict(
            "ASU hasn't finished the image yet".to_owned(),
        ));
    }
    // ASU names the release it built (`version_number`); its image names carry it too.
    let marker = format!("-{release}-");
    let of_release = match &build.release {
        Some(version) => version == release,
        None => build
            .images
            .iter()
            .any(|image| image.name.contains(&marker)),
    };
    if !of_release {
        return Err(ApiError::Conflict(format!(
            "ASU's build {hash} isn't of {release}"
        )));
    }
    Ok(build.images)
}

/// An upload's kind by its name: x86's `combined-efi` or `combined`, else `sysupgrade`.
fn kind_of(name: &str) -> &'static str {
    if name.contains("combined-efi") {
        "combined-efi"
    } else if name.contains("combined") {
        "combined"
    } else {
        "sysupgrade"
    }
}

/// Where the job's image comes from.
enum Fetch {
    /// Downloaded from its URL into the cache, or copied from the feed mirror's (D-147).
    Cache(ImageCache, Option<FeedMirror>),
    /// An uploaded image's file.
    Upload(PathBuf),
    /// A GitHub release's asset, with the token kept for private repos.
    GitHub {
        github: GitHub,
        cache: ImageCache,
        repo: String,
        tag: String,
        token: Option<Zeroizing<String>>,
    },
}

impl FetchImage for Fetch {
    fn fetch<'a>(&'a self, image: &'a FirmwareImage) -> BoxFuture<'a, Result<PathBuf, String>> {
        Box::pin(async move {
            match self {
                Self::Cache(cache, mirror) => {
                    if let Some(kept) = mirror.as_ref().and_then(|mirror| mirror.image(image)) {
                        match cache.fetch_local(image, &kept).await {
                            Ok(path) => return Ok(path),
                            Err(err) => tracing::warn!(
                                "the feed mirror's {} can't be used: {err}",
                                image.name
                            ),
                        }
                    }
                    cache.fetch(image).await.map_err(|err| err.to_string())
                }
                Self::Upload(path) => match tokio::fs::metadata(path).await {
                    Ok(_) => Ok(path.clone()),
                    Err(_) => Err("the uploaded image was deleted".to_owned()),
                },
                Self::GitHub {
                    github,
                    cache,
                    repo,
                    tag,
                    token,
                } => github
                    .fetch_image(
                        cache,
                        repo,
                        tag,
                        &image.name,
                        token.as_deref().map(String::as_str),
                    )
                    .await
                    .map_err(|err| err.to_string()),
            }
        })
    }
}
