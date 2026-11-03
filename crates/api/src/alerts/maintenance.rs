//! Maintenance windows, with a session (features/observability.md, Alerts): alerts on their
//! nodes (every node's when they name none) are silenced while they are on.
//!
//! - `GET /api/v1/maintenance`: the windows on now or to come, by start.
//! - `POST /api/v1/maintenance` (admins; audited): schedules one ([`NewWindow`]; a snooze is a
//!   window from now).
//! - `DELETE /api/v1/maintenance/{id}` (admins; audited): deletes one, ending it when it is on.

use std::time::Duration;

use axum::{
    Json,
    extract::{Path, State, rejection::JsonRejection},
    http::StatusCode,
};
use serde::Serialize;
use ts_rs::TS;
use uniwrt_core::WindowId;
use uniwrt_model::{
    alerts::{MaintenanceWindow, NewWindow},
    user::AuditAction,
};
use uniwrt_store::AlertRepo as _;

use super::{admin, audit, reload};
use crate::{Api, ApiError, ApiResult, Client, CurrentUser};

/// The longest window, and how far ahead one may start.
const MAX_LENGTH: Duration = Duration::from_hours(30 * 24);
const MAX_AHEAD: Duration = Duration::from_hours(365 * 24);
const MAX_NOTE: usize = 200;
const MAX_NODES: usize = 32;
const MAX_WINDOWS: usize = 100;

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct WindowList {
    pub windows: Vec<MaintenanceWindow>,
}

pub(crate) async fn list(State(api): State<Api>, _: CurrentUser) -> ApiResult<Json<WindowList>> {
    let windows = api.store().maintenance_windows(api.now()).await?;
    Ok(Json(WindowList { windows }))
}

pub(crate) async fn create(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    body: Result<Json<NewWindow>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<MaintenanceWindow>)> {
    admin(&user, "schedule maintenance")?;
    let Json(mut window) = body?;
    let now = api.now();
    window.note = window.note.trim().to_owned();
    let bad = |detail: &str| Err(ApiError::BadRequest(detail.to_owned()));
    if window.ends_at <= window.starts_at {
        return bad("A window ends after it starts");
    }
    if window.ends_at <= now {
        return bad("A window ends in the future");
    }
    if window.ends_at.saturating_duration_since(window.starts_at) > MAX_LENGTH {
        return bad("A window lasts at most 30 days");
    }
    if window.starts_at.saturating_duration_since(now) > MAX_AHEAD {
        return bad("A window starts within a year");
    }
    if window.note.chars().count() > MAX_NOTE || window.note.contains(char::is_control) {
        return Err(ApiError::BadRequest(format!(
            "A note has at most {MAX_NOTE} characters, on one line"
        )));
    }
    window.nodes.sort_unstable();
    window.nodes.dedup();
    if window.nodes.len() > MAX_NODES {
        return Err(ApiError::BadRequest(format!(
            "A window names at most {MAX_NODES} devices"
        )));
    }
    if api.store().maintenance_windows(now).await?.len() >= MAX_WINDOWS {
        return Err(ApiError::Conflict(format!(
            "At most {MAX_WINDOWS} windows are on or to come"
        )));
    }
    let window = api
        .store()
        .create_maintenance_window(window, Some(user.user.id))
        .await?;
    reload(&api).await;
    let nodes = if window.nodes.is_empty() {
        "every device".to_owned()
    } else {
        format!("devices {:?}", window.nodes)
    };
    let detail = format!(
        "window {} from {} to {} for {nodes}",
        window.id, window.starts_at, window.ends_at
    );
    audit(&api, &user, &client, AuditAction::Maintenance, detail).await?;
    Ok((StatusCode::CREATED, Json(window)))
}

pub(crate) async fn delete(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    admin(&user, "schedule maintenance")?;
    let id = WindowId::new(id);
    if !api.store().delete_maintenance_window(id).await? {
        return Err(ApiError::NotFound(format!(
            "There is no maintenance window {id}"
        )));
    }
    reload(&api).await;
    let detail = format!("deleted window {id}");
    audit(&api, &user, &client, AuditAction::Maintenance, detail).await?;
    Ok(StatusCode::NO_CONTENT)
}
