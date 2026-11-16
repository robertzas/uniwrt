//! Uploaded firmware images (M7.1a2, D-122), for admins; listed by `GET v1/firmware`:
//!
//! - `POST /api/v1/firmware/uploads?name=<file name>` (the image as the body, at most
//!   [`MAX_UPLOAD_BYTES`]; [`FirmwareUpload`], 201): streamed to the data dir while hashed, then
//!   kept once its metadata names the boards it fits (an x86 image: its name, its target).
//!   Anything else is refused (400) and nothing is kept. Audited (`firmware`).
//! - `DELETE /api/v1/firmware/uploads/{sha256}` (204): audited.

use axum::{
    Json,
    body::Body,
    extract::{
        DefaultBodyLimit, Path, Query, State,
        rejection::{PathRejection, QueryRejection},
    },
    http::StatusCode,
};
use http_body_util::BodyExt as _;
use serde::Deserialize;
use uniwrt_firmware::{Error, MAX_UPLOAD_BYTES};
use uniwrt_model::{
    firmware::FirmwareUpload,
    user::{AuditAction, AuditEntry, User},
};
use uniwrt_store::AuditRepo as _;

use crate::{
    Api, ApiError, ApiResult, Client, CurrentUser,
    firmware::{admin, firmware_error},
};

pub(crate) fn routes() -> axum::Router<Api> {
    use axum::routing::{delete, post};
    let limit = usize::try_from(MAX_UPLOAD_BYTES).unwrap_or(usize::MAX);
    axum::Router::new()
        .route(
            "/v1/firmware/uploads",
            post(upload).layer(DefaultBodyLimit::max(limit)),
        )
        .route("/v1/firmware/uploads/{sha256}", delete(remove))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UploadQuery {
    /// The file's name: for people, and for an x86 image's target.
    name: String,
}

pub(crate) async fn upload(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    query: Result<Query<UploadQuery>, QueryRejection>,
    body: Body,
) -> ApiResult<(StatusCode, Json<FirmwareUpload>)> {
    let user = user.user;
    admin(&user)?;
    let Query(query) =
        query.map_err(|_| ApiError::BadRequest("An image needs its file name".to_owned()))?;
    let mut receiving = api
        .uploads()?
        .receive(&query.name)
        .await
        .map_err(upload_error)?;
    let mut body = body;
    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(|err| {
            ApiError::BadRequest(format!(
                "The image didn't arrive whole (at most {} MiB): {err}",
                MAX_UPLOAD_BYTES >> 20
            ))
        })?;
        if let Ok(data) = frame.into_data() {
            receiving.write(&data).await.map_err(upload_error)?;
        }
    }
    let upload = receiving.finish(api.now()).await.map_err(upload_error)?;
    let fits = if upload.boards.is_empty() {
        format!("any {} board", upload.target)
    } else {
        upload.boards.join(", ")
    };
    audit(
        &api,
        &user,
        &client,
        format!(
            "uploaded the firmware image {} ({}, {} bytes) for {fits}",
            upload.name, upload.sha256, upload.size_bytes
        ),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(upload)))
}

pub(crate) async fn remove(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    sha256: Result<Path<String>, PathRejection>,
) -> ApiResult<StatusCode> {
    let user = user.user;
    admin(&user)?;
    let Path(sha256) =
        sha256.map_err(|_| ApiError::BadRequest("An image is named by its SHA-256".to_owned()))?;
    let uploads = api.uploads()?;
    let Some(upload) = uploads.get(&sha256).await.map_err(firmware_error)? else {
        return Err(ApiError::NotFound(format!(
            "There is no uploaded image {sha256}"
        )));
    };
    uploads.remove(&sha256).await.map_err(firmware_error)?;
    audit(
        &api,
        &user,
        &client,
        format!(
            "deleted the uploaded firmware image {} ({sha256})",
            upload.name
        ),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// An upload's errors: what the image is (or isn't) is the client's to fix.
fn upload_error(err: Error) -> ApiError {
    match err {
        Error::Invalid(why) => ApiError::BadRequest(why),
        other => firmware_error(other),
    }
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
