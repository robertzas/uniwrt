//! Controller backups and restore (features/lifecycle.md, Backups; D-126, D-129):
//!
//! - `GET /api/v1/controller/backups` ([`ControllerBackupStatus`]), with a session.
//! - Admins: `POST /api/v1/controller/backups` (*Back up now*: answers 201 with the backup once
//!   taken, 409 without a passphrase or while one is taken), `GET …/backups/{name}` (the file,
//!   streamed, `no-store`) and `PUT …/backups/passphrase` ([`PassphraseBody`], at least
//!   [`MIN_PASSPHRASE`] characters, sealed in the vault; 204).
//! - Restore, by an admin or, on a new host, while first-run setup is open: `GET
//!   /api/v1/controller/restore` ([`RestoreState`]), `PUT …/restore` (the file as the body,
//!   kept as the upload; 201), `POST …/restore/unlock` ([`PassphraseBody`]: unpacked and checked;
//!   400 for a wrong passphrase or a file that isn't a backup, 409 for one of a newer uniwrt),
//!   `POST …/restore/apply` (the controller restarts and the start puts it in place; 202) and
//!   `DELETE …/restore` (204).
//!
//! Every admin action is audited (`backup`). The upload is limited to [`MAX_UPLOAD_BYTES`].

use std::sync::Arc;

use axum::{
    Json,
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, Path, State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse as _, Response},
};
use http_body_util::BodyExt as _;
use serde::Deserialize;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use ts_rs::TS;
use uniwrt_model::{
    controller_backup::{
        AT_MIN, BackupOutcome, ControllerBackup, ControllerBackupStatus, MIN_PASSPHRASE,
        PASSPHRASE_SECRET, RestoreState,
    },
    user::{AuditAction, AuditEntry, Role, User},
};
use uniwrt_store::{
    AuditRepo as _, SettingsRepo as _, VaultRepo as _,
    controller_backup::{ControllerBackups, Staging},
};
use zeroize::Zeroizing;

use crate::{Api, ApiError, ApiResult, Client, CurrentUser, auth::setup::SetupState};

/// The largest backup an upload takes.
pub const MAX_UPLOAD_BYTES: u64 = 8 << 30;

/// What the server gives the API (D-129).
pub struct BackupConfig {
    pub backups: ControllerBackups,
    /// `<data dir>/restore`.
    pub staging: Staging,
    /// Stops the controller so that it restarts (under the launcher, or the container's policy).
    pub restart: Arc<dyn Fn() + Send + Sync>,
}

/// [`BackupConfig`], with one restore step at a time.
pub(crate) struct Backups {
    config: BackupConfig,
    restoring: tokio::sync::Mutex<()>,
}

impl Backups {
    pub(crate) fn new(config: BackupConfig) -> Self {
        Self {
            config,
            restoring: tokio::sync::Mutex::new(()),
        }
    }
}

/// A passphrase, to set or to open a backup with. Not `Debug`: it holds the passphrase.
#[derive(Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct PassphraseBody {
    pub passphrase: String,
}

pub(crate) fn routes() -> axum::Router<Api> {
    use axum::routing::{get, post, put};
    let limit = usize::try_from(MAX_UPLOAD_BYTES).unwrap_or(usize::MAX);
    axum::Router::new()
        .route("/v1/controller/backups", get(status).post(back_up))
        .route("/v1/controller/backups/passphrase", put(set_passphrase))
        .route("/v1/controller/backups/{name}", get(download))
        .route(
            "/v1/controller/restore",
            get(restore_state)
                .put(upload)
                .delete(discard)
                .layer(DefaultBodyLimit::max(limit)),
        )
        .route("/v1/controller/restore/unlock", post(unlock))
        .route("/v1/controller/restore/apply", post(apply))
}

fn admin(user: &User, what: &str) -> ApiResult<()> {
    if user.role == Role::Admin {
        Ok(())
    } else {
        Err(ApiError::Forbidden(format!("Only admins {what}")))
    }
}

/// Who may restore: an admin, or anyone while first-run setup is open (a new host, D-129); the
/// admin, if any, for the audit log.
async fn restorer(api: &Api, headers: &HeaderMap) -> ApiResult<Option<User>> {
    if api.setup().state().0 == SetupState::Open {
        return Ok(None);
    }
    let user = match crate::auth::session::presented(headers) {
        Some(token) => crate::auth::session::current(api, token).await?,
        None => None,
    }
    .ok_or_else(|| ApiError::Unauthorized("Log in first".to_owned()))?
    .user;
    admin(&user, "restore the controller")?;
    Ok(Some(user))
}

async fn audit(api: &Api, user: Option<&User>, client: &Client, detail: String) -> ApiResult<()> {
    api.store()
        .record(AuditEntry {
            at: api.now(),
            action: AuditAction::Backup,
            user_id: user.map(|user| user.id),
            username: user.map(|user| user.username.clone()),
            ip: Some(client.ip.to_string()),
            detail: Some(detail),
        })
        .await?;
    Ok(())
}

pub(crate) async fn status(
    State(api): State<Api>,
    _user: CurrentUser,
) -> ApiResult<Json<ControllerBackupStatus>> {
    let backups = api.controller_backups()?;
    let dirs = &backups.config.backups;
    let last = api
        .store()
        .setting(BackupOutcome::KEY)
        .await?
        .and_then(|value| serde_json::from_value(value).ok());
    Ok(Json(ControllerBackupStatus {
        passphrase_set: api.vault()?.secret(PASSPHRASE_SECRET).await?.is_some(),
        dir: dirs.dir().display().to_string(),
        extra_dir: dirs.extra().map(|dir| dir.display().to_string()),
        backups: dirs.list().await?,
        running: dirs.running(),
        last,
        restore: backups.config.staging.state().await,
        at_min: AT_MIN,
    }))
}

pub(crate) async fn back_up(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
) -> ApiResult<(StatusCode, Json<ControllerBackup>)> {
    let user = user.user;
    admin(&user, "back up the controller")?;
    let dirs = api.controller_backups()?.config.backups.clone();
    let (store, vault, version) = (
        api.store().clone(),
        api.vault()?.clone(),
        api.build().version,
    );
    // Its own task: a closed connection doesn't cut a backup short.
    let taken = tokio::spawn(async move { dirs.run(&store, &vault, version).await })
        .await
        .map_err(|_| ApiError::Internal("the backup's task ended".to_owned()))?
        .map_err(|err| match err {
            uniwrt_store::Error::NotFound { .. } => {
                ApiError::Conflict("Set the backup passphrase first".to_owned())
            }
            uniwrt_store::Error::Conflict { .. } => {
                ApiError::Conflict("A backup is being taken already".to_owned())
            }
            err => err.into(),
        })?;
    audit(
        &api,
        Some(&user),
        &client,
        format!("backed up the controller as {}", taken.backup.name),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(taken.backup)))
}

pub(crate) async fn set_passphrase(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    body: Result<Json<PassphraseBody>, JsonRejection>,
) -> ApiResult<StatusCode> {
    let user = user.user;
    admin(&user, "set the backup passphrase")?;
    let Json(body) = body?;
    let passphrase = Zeroizing::new(body.passphrase);
    if passphrase.chars().count() < MIN_PASSPHRASE {
        return Err(ApiError::BadRequest(format!(
            "A backup passphrase has at least {MIN_PASSPHRASE} characters"
        )));
    }
    api.controller_backups()?;
    api.vault()?
        .put_secret(
            PASSPHRASE_SECRET,
            Zeroizing::new(passphrase.as_bytes().to_vec()),
        )
        .await?;
    audit(
        &api,
        Some(&user),
        &client,
        "set the controller backup passphrase".to_owned(),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn download(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    Path(name): Path<String>,
) -> ApiResult<Response> {
    let user = user.user;
    admin(&user, "download controller backups")?;
    let path = api
        .controller_backups()?
        .config
        .backups
        .file(&name)
        .ok_or_else(|| ApiError::NotFound(format!("There is no backup {name}")))?;
    let file = tokio::fs::File::open(&path)
        .await
        .map_err(|err| ApiError::Internal(format!("cannot open {}: {err}", path.display())))?;
    let len = file.metadata().await.map_or(0, |meta| meta.len());
    audit(
        &api,
        Some(&user),
        &client,
        format!("downloaded the controller backup {name}"),
    )
    .await?;
    let chunks = futures_util::stream::try_unfold(file, |mut file| async move {
        let mut chunk = vec![0; 64 * 1024];
        let read = file.read(&mut chunk).await?;
        chunk.truncate(read);
        Ok::<_, std::io::Error>((read > 0).then(|| (Bytes::from(chunk), file)))
    });
    Ok((
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_owned()),
            (header::CONTENT_LENGTH, len.to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{name}\""),
            ),
            (header::CACHE_CONTROL, "no-store".to_owned()),
        ],
        Body::from_stream(chunks),
    )
        .into_response())
}

pub(crate) async fn restore_state(
    State(api): State<Api>,
    headers: HeaderMap,
) -> ApiResult<Json<RestoreState>> {
    let backups = api.controller_backups()?;
    restorer(&api, &headers).await?;
    Ok(Json(backups.config.staging.state().await))
}

pub(crate) async fn upload(
    State(api): State<Api>,
    headers: HeaderMap,
    client: Client,
    body: Body,
) -> ApiResult<(StatusCode, Json<RestoreState>)> {
    let backups = api.controller_backups()?;
    let user = restorer(&api, &headers).await?;
    let _one = backups.restoring.try_lock().map_err(|_| busy())?;
    let staging = &backups.config.staging;
    staging.discard().await;
    let path = staging.upload_path()?;
    let part = path.with_extension("age.part");
    let written = async {
        let mut file = tokio::fs::File::create(&part).await?;
        let mut body = body;
        while let Some(frame) = body.frame().await {
            let frame = frame.map_err(std::io::Error::other)?;
            if let Ok(data) = frame.into_data() {
                file.write_all(&data).await?;
            }
        }
        file.sync_all().await?;
        tokio::fs::rename(&part, &path).await
    }
    .await;
    if let Err(err) = written {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(ApiError::BadRequest(format!(
            "The backup didn't arrive whole (at most {} GiB): {err}",
            MAX_UPLOAD_BYTES >> 30
        )));
    }
    let state = staging.state().await;
    let size = match &state {
        RestoreState::Uploaded { size_bytes } => *size_bytes,
        _ => 0,
    };
    audit(
        &api,
        user.as_ref(),
        &client,
        format!("uploaded a controller backup to restore ({size} bytes)"),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(state)))
}

pub(crate) async fn unlock(
    State(api): State<Api>,
    headers: HeaderMap,
    client: Client,
    body: Result<Json<PassphraseBody>, JsonRejection>,
) -> ApiResult<Json<RestoreState>> {
    let backups = api.controller_backups()?;
    let user = restorer(&api, &headers).await?;
    let Json(body) = body?;
    let passphrase = Zeroizing::new(body.passphrase);
    let _one = backups.restoring.try_lock().map_err(|_| busy())?;
    let staging = &backups.config.staging;
    if !matches!(staging.state().await, RestoreState::Uploaded { .. }) {
        return Err(ApiError::Conflict("Upload a backup first".to_owned()));
    }
    let manifest = staging
        .unlock(passphrase.as_bytes())
        .await
        .map_err(restore_error)?;
    audit(
        &api,
        user.as_ref(),
        &client,
        format!(
            "staged the controller backup taken at {} by uniwrt {}",
            manifest.taken_at, manifest.version
        ),
    )
    .await?;
    Ok(Json(RestoreState::Ready { manifest }))
}

pub(crate) async fn apply(
    State(api): State<Api>,
    headers: HeaderMap,
    client: Client,
) -> ApiResult<StatusCode> {
    let backups = api.controller_backups()?;
    let user = restorer(&api, &headers).await?;
    let _one = backups.restoring.try_lock().map_err(|_| busy())?;
    let RestoreState::Ready { manifest } = backups.config.staging.state().await else {
        return Err(ApiError::Conflict(
            "Unlock an uploaded backup first".to_owned(),
        ));
    };
    audit(
        &api,
        user.as_ref(),
        &client,
        format!(
            "restarted to restore the controller backup taken at {}",
            manifest.taken_at
        ),
    )
    .await?;
    (backups.config.restart)();
    Ok(StatusCode::ACCEPTED)
}

pub(crate) async fn discard(
    State(api): State<Api>,
    headers: HeaderMap,
    client: Client,
) -> ApiResult<StatusCode> {
    let backups = api.controller_backups()?;
    let user = restorer(&api, &headers).await?;
    let _one = backups.restoring.try_lock().map_err(|_| busy())?;
    if backups.config.staging.state().await != RestoreState::None {
        backups.config.staging.discard().await;
        audit(
            &api,
            user.as_ref(),
            &client,
            "discarded the staged controller restore".to_owned(),
        )
        .await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

fn busy() -> ApiError {
    ApiError::Conflict("Another restore step is under way".to_owned())
}

fn restore_error(err: uniwrt_store::Error) -> ApiError {
    use uniwrt_store::Error;
    match err {
        Error::Passphrase => {
            ApiError::BadRequest("The passphrase doesn't open this backup".to_owned())
        }
        Error::NotAge(why) | Error::NotBackup(why) => {
            ApiError::BadRequest(format!("This isn't a uniwrt controller backup: {why}"))
        }
        Error::TooNew { found, .. } => ApiError::Conflict(format!(
            "This backup is of a newer uniwrt (database schema {found}): upgrade the controller first"
        )),
        Error::WrongMasterKey { .. } => {
            ApiError::BadRequest("The backup's master key doesn't open its database".to_owned())
        }
        err => err.into(),
    }
}
