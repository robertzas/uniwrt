//! Zero-touch images built by ASU (features/adoption.md, Path B; D-022, D-104), for admins:
//!
//! - `GET /api/v1/ztp/releases` ([`FirmwareReleases`]): each supported branch's latest
//!   release, the first the default.
//! - `GET /api/v1/ztp/profiles?release=` ([`FirmwareProfile`]s): the release's devices.
//! - `POST /api/v1/ztp/builds` ([`ImageBuildBody`]; [`ImageBuildStarted`]): asks ASU for an
//!   image of the device with the mesh essentials and the first-boot script of the role, which
//!   calls home where the admin reached the controller. Audited (`ztp`).
//! - `GET /api/v1/ztp/builds/{hash}` ([`ImageBuild`]): how a build stands; its images are
//!   downloaded from ASU's store.
//!
//! The controller only relays: ASU and OpenWrt's download server are public, and nothing
//! secret goes to them (D-022).

use axum::{
    Json,
    extract::{
        Path, Query, State,
        rejection::{JsonRejection, PathRejection, QueryRejection},
    },
};
use serde::Deserialize;
use ts_rs::TS;
use uniwrt_firmware::{Error, ImageRequest};
use uniwrt_model::{
    firmware::{FirmwareProfile, FirmwareReleases, ImageBuild, ImageBuildStarted},
    node::NodeRole,
};

use crate::{Api, ApiError, ApiResult, Client, CurrentUser};

/// An image to build.
#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct ImageBuildBody {
    /// One of `GET v1/ztp/releases`.
    pub release: String,
    /// The device's target and profile, as `GET v1/ztp/profiles` lists them.
    pub target: String,
    pub profile: String,
    /// The role it calls home as: an access point when left out.
    #[serde(default)]
    #[ts(optional)]
    pub role: Option<NodeRole>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProfilesQuery {
    release: String,
}

pub(crate) async fn releases(
    State(api): State<Api>,
    user: CurrentUser,
) -> ApiResult<Json<FirmwareReleases>> {
    super::admin(&user.user, "build images")?;
    Ok(Json(api.asu()?.releases().await.map_err(firmware_error)?))
}

pub(crate) async fn profiles(
    State(api): State<Api>,
    user: CurrentUser,
    query: Result<Query<ProfilesQuery>, QueryRejection>,
) -> ApiResult<Json<Vec<FirmwareProfile>>> {
    super::admin(&user.user, "build images")?;
    let Query(query) = query.map_err(|err| ApiError::BadRequest(err.body_text()))?;
    let profiles = api
        .asu()?
        .profiles(&query.release)
        .await
        .map_err(firmware_error)?;
    Ok(Json(profiles.to_vec()))
}

pub(crate) async fn build(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    body: Result<Json<ImageBuildBody>, JsonRejection>,
) -> ApiResult<Json<ImageBuildStarted>> {
    let user = user.user;
    super::admin(&user, "build images")?;
    let Json(body) = body?;
    let asu = api.asu()?;
    let device = asu
        .profile(&body.release, &body.target, &body.profile)
        .await
        .map_err(firmware_error)?;
    let role = body.role.unwrap_or(NodeRole::Ap);
    let script = super::script::first_boot(&api, &client, role).await?;
    let started = asu
        .build(ImageRequest {
            release: &body.release,
            target: &body.target,
            profile: &body.profile,
            script: &script,
        })
        .await
        .map_err(firmware_error)?;
    let title = device
        .titles
        .first()
        .map_or(device.id.as_str(), String::as_str);
    super::audit(
        &api,
        &user,
        &client,
        format!(
            "asked ASU for an image of the {title} on {} as {} ({})",
            body.release,
            role_name(role),
            started.build.hash
        ),
    )
    .await?;
    Ok(Json(started))
}

pub(crate) async fn status(
    State(api): State<Api>,
    user: CurrentUser,
    hash: Result<Path<String>, PathRejection>,
) -> ApiResult<Json<ImageBuild>> {
    super::admin(&user.user, "build images")?;
    let Path(hash) = hash.map_err(|_| ApiError::BadRequest("A build is needed".to_owned()))?;
    Ok(Json(
        api.asu()?.status(&hash).await.map_err(firmware_error)?,
    ))
}

const fn role_name(role: NodeRole) -> &'static str {
    match role {
        NodeRole::Gateway => "a gateway",
        NodeRole::Ap => "an access point",
        NodeRole::Bridge => "a bridge",
    }
}

fn firmware_error(err: Error) -> ApiError {
    match err {
        Error::NotFound(why) => ApiError::NotFound(why),
        Error::Busy(_) => ApiError::Unavailable(err.to_string()),
        Error::Unreachable { .. }
        | Error::Status { .. }
        | Error::Invalid(_)
        | Error::Refused(_) => ApiError::BadGateway(err.to_string()),
    }
}
