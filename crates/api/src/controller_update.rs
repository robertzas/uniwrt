//! The controller's own update (features/lifecycle.md, Firmware; M7.1b2, D-123):
//!
//! - `GET /api/v1/controller/update` ([`ControllerUpdate`]): the running version, the newest
//!   release of uniwrt's repo for this binary's target that is newer and not rejected by a
//!   failed trial (or why GitHub can't say), why this controller can't install it itself (a
//!   development build, demo mode, self-upgrade off), an install's progress, the last failed
//!   trial and the automatic install's setting.
//! - `POST /api/v1/controller/update` ([`InstallBody`]), for admins: installs the release on
//!   offer, named so an admin installs what they saw. It runs in the background through
//!   [`SelfUpdate::install`]: staged and verified, run once, named the trial, then the
//!   controller restarts into it under the launcher's watch. Answers 202. Audited
//!   (`controller`).
//! - `PUT /api/v1/controller/update/settings` ([`ControllerUpdates`]), for admins: the nightly
//!   automatic install (off by default; the window's start). Audited.

use std::sync::Arc;

use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
};
use serde::Deserialize;
use ts_rs::TS;
use uniwrt_firmware::{CONTROLLER_REPO, ControllerRelease, Version};
use uniwrt_model::{
    controller::{
        ControllerOffer, ControllerUpdate, ControllerUpdates, InstallProgress, TrialFailure,
    },
    user::{AuditAction, AuditEntry, Role, User},
};
use uniwrt_store::{AuditRepo as _, SettingsRepo as _, Store};

use crate::{Api, ApiError, ApiResult, Client, CurrentUser};

/// The controller's own install, as the server's launcher provides it (D-123).
pub trait SelfUpdate: Send + Sync + 'static {
    /// The target triple this binary was built for: the tarball to take.
    fn target(&self) -> &'static str;
    /// Why this controller can't install a release itself, when it can't.
    fn blocked(&self) -> Option<String>;
    /// What the launcher keeps of past trials.
    fn trials(&self) -> Trials;
    fn progress(&self) -> InstallProgress;
    /// Starts installing `release` in the background; refused, with why, when blocked or while
    /// another install runs.
    fn install(&self, release: ControllerRelease) -> Result<(), String>;
}

/// The launcher's record of trials.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Trials {
    /// The last release that failed its trial.
    pub failure: Option<TrialFailure>,
    /// The releases that failed theirs: never offered again.
    pub rejected: Vec<Version>,
}

pub(crate) fn routes() -> axum::Router<Api> {
    use axum::routing::{get, put};
    axum::Router::new()
        .route("/v1/controller/update", get(status).post(install))
        .route("/v1/controller/update/settings", put(put_settings))
}

/// The release an admin installs.
#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct InstallBody {
    /// The version on offer, `X.Y.Z`.
    pub version: String,
}

async fn status(State(api): State<Api>, _user: CurrentUser) -> ApiResult<Json<ControllerUpdate>> {
    let updater = api.self_update()?;
    let (offer, note) = newest(&api, updater.as_ref()).await;
    Ok(Json(
        view(&api, updater.as_ref(), offer.as_ref(), note).await?,
    ))
}

async fn install(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    body: Result<Json<InstallBody>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<ControllerUpdate>)> {
    let user = user.user;
    admin(&user)?;
    let Json(body) = body?;
    let updater = api.self_update()?;
    if let Some(why) = updater.blocked() {
        return Err(ApiError::Conflict(why));
    }
    if updater.progress().busy() {
        return Err(ApiError::Conflict(
            "An install is already under way".to_owned(),
        ));
    }
    let (offer, note) = newest(&api, updater.as_ref()).await;
    let Some(release) = offer else {
        return Err(match note {
            Some(note) => ApiError::BadGateway(note),
            None => ApiError::Conflict("This controller runs the newest release".to_owned()),
        });
    };
    if release.version.to_string() != body.version {
        return Err(ApiError::Conflict(format!(
            "{} isn't the release on offer: {} is (reload the page)",
            body.version, release.version
        )));
    }
    let detail = format!(
        "installing the controller's release {} (running {})",
        release.version,
        api.build().version
    );
    updater
        .install(release.clone())
        .map_err(ApiError::Conflict)?;
    audit(&api, &user, &client, detail).await?;
    let view = view(&api, updater.as_ref(), Some(&release), None).await?;
    Ok((StatusCode::ACCEPTED, Json(view)))
}

async fn put_settings(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    body: Result<Json<ControllerUpdates>, JsonRejection>,
) -> ApiResult<Json<ControllerUpdates>> {
    let user = user.user;
    admin(&user)?;
    let Json(settings) = body?;
    if settings.at_min >= 24 * 60 {
        return Err(ApiError::BadRequest(
            "The window opens at a time of day: 00:00 to 23:59".to_owned(),
        ));
    }
    let value =
        serde_json::to_value(settings).map_err(|err| ApiError::Internal(err.to_string()))?;
    api.store()
        .put_setting(ControllerUpdates::KEY, value)
        .await?;
    let detail = if settings.auto {
        format!(
            "turned the controller's automatic install on, nightly at {:02}:{:02}",
            settings.at_min / 60,
            settings.at_min % 60
        )
    } else {
        "turned the controller's automatic install off".to_owned()
    };
    audit(&api, &user, &client, detail).await?;
    Ok(Json(settings))
}

/// The automatic install's setting as kept; the defaults when none is.
pub async fn settings(store: &Store) -> uniwrt_store::Result<ControllerUpdates> {
    Ok(store
        .setting(ControllerUpdates::KEY)
        .await?
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default())
}

/// The newest release for this target that is newer than the running one and not rejected,
/// or why GitHub can't say (in a sentence; the error is logged).
async fn newest(
    api: &Api,
    updater: &dyn SelfUpdate,
) -> (Option<ControllerRelease>, Option<String>) {
    let github = match api.github() {
        Ok(github) => github,
        Err(err) => return (None, Some(err.to_string())),
    };
    match github.releases(CONTROLLER_REPO, None).await {
        Ok(releases) => {
            let current = api.build().version.parse::<Version>().ok();
            let rejected = updater.trials().rejected;
            let offer = releases.controller_where(updater.target(), |release| {
                current.is_none_or(|current| release.version > current)
                    && !rejected.contains(&release.version)
            });
            (offer, None)
        }
        Err(err) => {
            tracing::warn!(%err, "cannot read uniwrt's releases from GitHub");
            (
                None,
                Some("Cannot read uniwrt's releases from GitHub now: try again later".to_owned()),
            )
        }
    }
}

async fn view(
    api: &Api,
    updater: &dyn SelfUpdate,
    offer: Option<&ControllerRelease>,
    note: Option<String>,
) -> ApiResult<ControllerUpdate> {
    Ok(ControllerUpdate {
        current: api.build().version.to_owned(),
        offer: offer.map(|release| ControllerOffer {
            version: release.version.to_string(),
            name: release.name.clone(),
            notes: release.notes.clone(),
            published_at: release.published_at,
            size_bytes: release.size,
        }),
        note,
        blocked: updater.blocked(),
        progress: updater.progress(),
        failure: updater.trials().failure,
        settings: settings(api.store()).await?,
    })
}

fn admin(user: &User) -> ApiResult<()> {
    if user.role == Role::Admin {
        Ok(())
    } else {
        Err(ApiError::Forbidden(
            "Only admins update the controller".to_owned(),
        ))
    }
}

async fn audit(api: &Api, user: &User, client: &Client, detail: String) -> ApiResult<()> {
    api.store()
        .record(AuditEntry {
            at: api.now(),
            action: AuditAction::Controller,
            user_id: Some(user.id),
            username: Some(user.username.clone()),
            ip: Some(client.ip.to_string()),
            detail: Some(detail),
        })
        .await?;
    Ok(())
}

/// The updater as the API holds it.
pub(crate) type Updater = Arc<dyn SelfUpdate>;
