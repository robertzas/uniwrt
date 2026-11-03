//! The alert rules, with a session (features/observability.md, Alerts):
//!
//! - `GET /api/v1/alert-rules`: every rule ([`RuleView`]: built-in ones say whether they were
//!   changed).
//! - `POST /api/v1/alert-rules` and `PUT /api/v1/alert-rules/{id}` (admins; audited): a
//!   [`RuleSpec`] creates or replaces one.
//! - `DELETE /api/v1/alert-rules/{id}` (admins; audited): deletes one of the user's; built-in
//!   rules are disabled instead (409).
//! - `POST /api/v1/alert-rules/{id}/reset` (admins; audited): a built-in rule as it shipped.
//!
//! Each change has the alert engine read the rules again before answering.

use axum::{
    Json,
    extract::{Path, State, rejection::JsonRejection},
    http::StatusCode,
};
use serde::Serialize;
use ts_rs::TS;
use uniwrt_core::RuleId;
use uniwrt_events::alerts::Preset;
use uniwrt_model::{
    alerts::{AlertRule, RuleSpec},
    user::AuditAction,
};
use uniwrt_store::AlertRepo as _;

use super::{admin, audit, reload};
use crate::{Api, ApiError, ApiResult, Client, CurrentUser};

/// The most rules a controller keeps: each is evaluated against every window and event.
const MAX_RULES: usize = 200;

/// A rule, and whether a built-in one differs from what shipped.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RuleView {
    #[serde(flatten)]
    pub rule: AlertRule,
    pub modified: bool,
}

/// Every rule, by id.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RuleList {
    pub rules: Vec<RuleView>,
}

fn presets() -> Vec<Preset> {
    uniwrt_events::presets().unwrap_or_default()
}

fn view(rule: AlertRule, presets: &[Preset]) -> RuleView {
    let modified = rule.preset.as_ref().is_some_and(|id| {
        presets
            .iter()
            .find(|preset| preset.id == *id)
            .is_none_or(|preset| preset.spec != rule.spec)
    });
    RuleView { rule, modified }
}

pub(crate) async fn list(State(api): State<Api>, _: CurrentUser) -> ApiResult<Json<RuleList>> {
    let presets = presets();
    let rules = api
        .store()
        .alert_rules()
        .await?
        .into_iter()
        .map(|rule| view(rule, &presets))
        .collect();
    Ok(Json(RuleList { rules }))
}

fn check(spec: &mut RuleSpec) -> ApiResult<()> {
    spec.name = spec.name.trim().to_owned();
    spec.validate().map_err(ApiError::BadRequest)
}

pub(crate) async fn create(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    body: Result<Json<RuleSpec>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<RuleView>)> {
    admin(&user, "change alert rules")?;
    let Json(mut spec) = body?;
    check(&mut spec)?;
    if api.store().alert_rules().await?.len() >= MAX_RULES {
        return Err(ApiError::Conflict(format!(
            "A controller keeps at most {MAX_RULES} alert rules"
        )));
    }
    let rule = api.store().create_alert_rule(spec).await?;
    reload(&api).await;
    let detail = format!("created rule {} ({:?})", rule.id, rule.spec.name);
    audit(&api, &user, &client, AuditAction::AlertRule, detail).await?;
    Ok((StatusCode::CREATED, Json(view(rule, &[]))))
}

pub(crate) async fn update(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    Path(id): Path<i64>,
    body: Result<Json<RuleSpec>, JsonRejection>,
) -> ApiResult<Json<RuleView>> {
    admin(&user, "change alert rules")?;
    let Json(mut spec) = body?;
    check(&mut spec)?;
    let id = RuleId::new(id);
    let rule = api
        .store()
        .update_alert_rule(id, spec)
        .await?
        .ok_or_else(|| unknown(id))?;
    reload(&api).await;
    let detail = format!(
        "edited rule {id} ({:?}, {})",
        rule.spec.name,
        if rule.spec.enabled {
            "enabled"
        } else {
            "disabled"
        }
    );
    audit(&api, &user, &client, AuditAction::AlertRule, detail).await?;
    Ok(Json(view(rule, &presets())))
}

pub(crate) async fn delete(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    admin(&user, "change alert rules")?;
    let id = RuleId::new(id);
    let rule = api
        .store()
        .alert_rule(id)
        .await?
        .ok_or_else(|| unknown(id))?;
    if rule.preset.is_some() {
        return Err(ApiError::Conflict(
            "Built-in rules cannot be deleted; disable this one instead".to_owned(),
        ));
    }
    if !api.store().delete_alert_rule(id).await? {
        return Err(unknown(id));
    }
    reload(&api).await;
    let detail = format!("deleted rule {id} ({:?})", rule.spec.name);
    audit(&api, &user, &client, AuditAction::AlertRule, detail).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn reset(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    Path(id): Path<i64>,
) -> ApiResult<Json<RuleView>> {
    admin(&user, "change alert rules")?;
    let id = RuleId::new(id);
    let rule = api
        .store()
        .alert_rule(id)
        .await?
        .ok_or_else(|| unknown(id))?;
    let presets = presets();
    let preset = rule
        .preset
        .as_ref()
        .and_then(|name| presets.iter().find(|preset| preset.id == *name))
        .ok_or_else(|| {
            ApiError::Conflict("Only built-in rules can be reset; this one is yours".to_owned())
        })?;
    let rule = api
        .store()
        .update_alert_rule(id, preset.spec.clone())
        .await?
        .ok_or_else(|| unknown(id))?;
    reload(&api).await;
    let detail = format!("reset rule {id} ({:?})", rule.spec.name);
    audit(&api, &user, &client, AuditAction::AlertRule, detail).await?;
    Ok(Json(view(rule, &presets)))
}

fn unknown(id: RuleId) -> ApiError {
    ApiError::NotFound(format!("There is no alert rule {id}"))
}
