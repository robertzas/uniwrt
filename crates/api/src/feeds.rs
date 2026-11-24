//! The package feed mirror (features/lifecycle.md, Offline packages; D-147):
//!
//! - `GET /api/v1/feeds`: its settings and what it holds ([`FeedsView`]).
//! - `PUT /api/v1/feeds/settings` (admins): the settings ([`MirrorSettings`]), in force at once;
//!   the view. Audited.
//! - `POST /api/v1/feeds/refresh` (admins): every index asked for again now (conditionally);
//!   202 with the view, or 409 while the mirror is off. Audited.
//! - `GET /feeds/{*path}` ([`feed_files`]), without a session, on the HTTPS listener: what a
//!   node's package manager fetches, a planned feed's index or a package it lists (plain text
//!   404 for anything else, 502 when upstream can't give it now). Nodes check the feeds'
//!   signatures, so the controller's certificate isn't checked by them.

use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{Path, State},
    http::{StatusCode, header},
    response::{IntoResponse as _, Response},
};
use serde::Serialize;
use tokio::io::AsyncReadExt as _;
use ts_rs::TS;
use uniwrt_engine::FeedMirrorError;
use uniwrt_feeds::{ServeError, Served};
use uniwrt_model::{
    feeds::{MirrorSettings, MirrorStatus},
    user::AuditAction,
};
use uniwrt_store::SettingsRepo as _;

use crate::{
    Api, ApiError, ApiResult, Client, CurrentUser,
    alerts::{admin, audit},
};

/// The mirror's settings, and what it holds.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct FeedsView {
    pub settings: MirrorSettings,
    #[serde(flatten)]
    #[ts(flatten)]
    pub status: MirrorStatus,
}

fn view(feeds: &uniwrt_engine::FeedMirror) -> FeedsView {
    FeedsView {
        settings: feeds.settings(),
        status: feeds.status(),
    }
}

fn stopped(err: &FeedMirrorError) -> ApiError {
    match err {
        FeedMirrorError::Off => ApiError::Conflict(err.to_string()),
        FeedMirrorError::Stopped => ApiError::Unavailable(err.to_string()),
    }
}

pub(crate) async fn get_view(
    State(api): State<Api>,
    _user: CurrentUser,
) -> ApiResult<Json<FeedsView>> {
    Ok(Json(view(api.feeds()?)))
}

pub(crate) async fn put_settings(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    Json(settings): Json<MirrorSettings>,
) -> ApiResult<Json<FeedsView>> {
    admin(&user, "change the feed mirror")?;
    let feeds = api.feeds()?;
    let value = serde_json::to_value(settings)
        .map_err(|err| ApiError::Internal(format!("cannot encode the settings: {err}")))?;
    api.store().put_setting(MirrorSettings::KEY, value).await?;
    feeds.reload().await.map_err(|err| stopped(&err))?;
    let detail = format!(
        "set the feed mirror {}{}",
        if settings.enabled { "on" } else { "off" },
        if settings.whole_feeds {
            ", keeping whole feeds"
        } else {
            ""
        }
    );
    audit(&api, &user, &client, AuditAction::Packages, detail).await?;
    Ok(Json(view(feeds)))
}

pub(crate) async fn refresh(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
) -> ApiResult<(StatusCode, Json<FeedsView>)> {
    admin(&user, "refresh the feed mirror")?;
    let feeds = api.feeds()?;
    feeds.refresh().await.map_err(|err| stopped(&err))?;
    let detail = "refreshed the feed mirror".to_owned();
    audit(&api, &user, &client, AuditAction::Packages, detail).await?;
    Ok((StatusCode::ACCEPTED, Json(view(feeds))))
}

pub(crate) fn routes() -> Router<Api> {
    use axum::routing::{get, post, put};
    Router::new()
        .route("/v1/feeds", get(get_view))
        .route("/v1/feeds/settings", put(put_settings))
        .route("/v1/feeds/refresh", post(refresh))
}

/// The nodes' route to the mirror, to serve at the root of the HTTPS listener.
pub fn feed_files(api: Api) -> Router {
    Router::new()
        .route("/feeds/{*path}", axum::routing::get(serve))
        .with_state(api)
}

async fn serve(State(api): State<Api>, Path(path): Path<String>) -> Response {
    let text = |status: StatusCode, why: String| (status, format!("{why}\n")).into_response();
    let Ok(feeds) = api.feeds() else {
        return text(StatusCode::NOT_FOUND, ServeError::NotFound.to_string());
    };
    match feeds.serve(&path).await {
        Ok(Served { file, len }) => {
            let chunks = futures_util::stream::try_unfold(file, |mut file| async move {
                let mut chunk = vec![0; 64 * 1024];
                let read = file.read(&mut chunk).await?;
                chunk.truncate(read);
                Ok::<_, std::io::Error>((read > 0).then(|| (Bytes::from(chunk), file)))
            });
            (
                [
                    (header::CONTENT_TYPE, "application/octet-stream".to_owned()),
                    (header::CONTENT_LENGTH, len.to_string()),
                    (header::CACHE_CONTROL, "no-store".to_owned()),
                ],
                Body::from_stream(chunks),
            )
                .into_response()
        }
        Err(err @ ServeError::NotFound) => text(StatusCode::NOT_FOUND, err.to_string()),
        Err(ServeError::Unavailable(why)) => text(StatusCode::BAD_GATEWAY, why),
    }
}

#[cfg(test)]
#[path = "feeds_tests.rs"]
mod tests;
