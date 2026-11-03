//! The alerts routes, with a session (features/observability.md, Alerts):
//!
//! - `GET /api/v1/alerts?active=true&min=<level>&nodes=<id,…>&rules=<id,…>&from=<ms>&to=<ms>&
//!   before=<time>:<id>&limit=<n>`: alerts, newest first by start, a page at a time
//!   ([`AlertPage`]); the WS topic `alerts` keeps the active ones current.
//! - `POST /api/v1/alerts/{id}/acknowledge` (admins; audited): acknowledges one still firing.
//! - The rules (`rules`) and maintenance windows (`maintenance`).

pub(crate) mod maintenance;
pub(crate) mod rules;
#[cfg(test)]
mod tests;

use axum::{
    Json,
    extract::{Path, Query, State, rejection::QueryRejection},
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uniwrt_core::{AlertId, NodeId, RuleId, Timestamp};
use uniwrt_model::{
    alerts::{Acknowledgement, AlertFilter, AlertRecord},
    events::EventLevel,
    user::{AuditAction, AuditEntry, Role},
};
use uniwrt_store::{AlertRepo as _, AuditRepo as _};

use crate::{Api, ApiError, ApiResult, Client, CurrentUser};

const DEFAULT_LIMIT: usize = 100;
const MAX_LIMIT: usize = 1_000;
const MAX_VALUES: usize = 32;

/// One page of alerts, newest first.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct AlertPage {
    pub alerts: Vec<AlertRecord>,
    /// The `before` of the next page, when there may be one.
    pub next: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct AlertsQuery {
    active: Option<bool>,
    min: Option<String>,
    nodes: Option<String>,
    rules: Option<String>,
    from: Option<i64>,
    to: Option<i64>,
    before: Option<String>,
    limit: Option<usize>,
}

pub(crate) async fn list(
    State(api): State<Api>,
    _: CurrentUser,
    query: Result<Query<AlertsQuery>, QueryRejection>,
) -> ApiResult<Json<AlertPage>> {
    let Query(query) = query.map_err(|err| ApiError::BadRequest(err.body_text()))?;
    let ids = |value: Option<&str>, what: &str| -> ApiResult<Vec<i64>> {
        let values: Vec<&str> = value
            .unwrap_or_default()
            .split(',')
            .filter(|value| !value.is_empty())
            .collect();
        if values.len() > MAX_VALUES {
            return Err(ApiError::BadRequest(format!(
                "A filter names at most {MAX_VALUES} {what}"
            )));
        }
        values
            .into_iter()
            .map(|value| {
                value
                    .parse::<i64>()
                    .ok()
                    .filter(|id| *id > 0)
                    .ok_or_else(|| {
                        ApiError::BadRequest(format!("{value:?} is not one of the {what}"))
                    })
            })
            .collect()
    };
    let min = query
        .min
        .as_deref()
        .map(|name| {
            EventLevel::parse(name).ok_or_else(|| {
                ApiError::BadRequest(format!(
                    "{name:?} is not a level: use critical, warning, notice or info"
                ))
            })
        })
        .transpose()?;
    let filter = AlertFilter {
        active: query.active.unwrap_or(false),
        min,
        nodes: ids(query.nodes.as_deref(), "node IDs")?
            .into_iter()
            .map(NodeId::new)
            .collect(),
        rules: ids(query.rules.as_deref(), "rule IDs")?
            .into_iter()
            .map(RuleId::new)
            .collect(),
        from: query.from.map(Timestamp::from_unix_ms),
        to: query.to.map(Timestamp::from_unix_ms),
    };
    let before = query.before.as_deref().map(cursor).transpose()?;
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT);
    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(ApiError::BadRequest(format!(
            "`limit` is from 1 to {MAX_LIMIT}"
        )));
    }
    let alerts = api.store().alerts(filter, before, limit).await?;
    let next = (alerts.len() == limit)
        .then(|| alerts.last())
        .flatten()
        .map(|last| format!("{}:{}", last.state.started_at.as_unix_ms(), last.id));
    Ok(Json(AlertPage { alerts, next }))
}

fn cursor(text: &str) -> ApiResult<(Timestamp, AlertId)> {
    text.split_once(':')
        .and_then(|(at, id)| Some((at.parse().ok()?, id.parse().ok()?)))
        .map(|(at, id)| (Timestamp::from_unix_ms(at), AlertId::new(id)))
        .ok_or_else(|| ApiError::BadRequest(format!("{text:?} is not a page's cursor")))
}

pub(crate) async fn acknowledge(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    Path(id): Path<i64>,
) -> ApiResult<Json<AlertRecord>> {
    admin(&user, "acknowledge alerts")?;
    let id = AlertId::new(id);
    let ack = Acknowledgement {
        by: user.user.id,
        username: user.user.username.clone(),
        at: api.now(),
    };
    let alert = api
        .alerts()?
        .acknowledge(id, ack)
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?
        .ok_or_else(|| ApiError::NotFound(format!("There is no alert {id} still firing")))?;
    audit(
        &api,
        &user,
        &client,
        AuditAction::AcknowledgeAlert,
        format!(
            "alert {id} ({:?}, {})",
            alert.state.name, alert.state.subject
        ),
    )
    .await?;
    Ok(Json(alert))
}

/// Only admins change alerting.
pub(crate) fn admin(user: &CurrentUser, what: &str) -> ApiResult<()> {
    if user.user.role == Role::Admin {
        Ok(())
    } else {
        Err(ApiError::Forbidden(format!("Only admins {what}")))
    }
}

pub(crate) async fn audit(
    api: &Api,
    user: &CurrentUser,
    client: &Client,
    action: AuditAction,
    detail: String,
) -> ApiResult<()> {
    api.store()
        .record(AuditEntry {
            at: api.now(),
            action,
            user_id: Some(user.user.id),
            username: Some(user.user.username.clone()),
            ip: Some(client.ip.to_string()),
            detail: Some(detail),
        })
        .await?;
    Ok(())
}

/// Has the alert engine read the rules and windows again; a stopped engine reads them when it
/// starts.
pub(crate) async fn reload(api: &Api) {
    if let Ok(alerts) = api.alerts()
        && let Err(err) = alerts.reload().await
    {
        tracing::warn!(%err, "cannot have the alert engine reload its rules");
    }
}
