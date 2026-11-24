//! The WAN quality monitor's routes (features/observability.md, Troubleshooting assistants;
//! D-144): `GET v1/wan/monitor` is its settings, the latest round of probes and what it said of
//! the internet ([`WanMonitorView`]), for any signed-in user; an admin's `PUT
//! v1/wan/monitor` saves the settings ([`WanMonitorSettings`], checked; audited) and runs a
//! round at once. The rounds' series are the metrics' (`probe:<key>`), outages the events'
//! (`wan_outage`, `wan_restored`).

use std::net::IpAddr;

use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
};
use serde::Serialize;
use ts_rs::TS;
use uniwrt_core::{NodeId, Timestamp};
use uniwrt_model::{
    user::{AuditAction, AuditEntry, Role},
    wan_monitor::{ProbeRound, WanHealth, WanMonitorSettings},
};
use uniwrt_store::{AuditRepo as _, SettingsRepo as _};

use crate::{Api, ApiError, ApiResult, Client, CurrentUser};

/// The monitor as the UI shows it.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct WanMonitorView {
    pub settings: WanMonitorSettings,
    /// The gateway that runs the probes.
    #[ts(as = "Option<i64>")]
    pub gateway: Option<NodeId>,
    /// What the latest round said; `None` before the first.
    pub health: Option<WanHealth>,
    /// Since when it says so, as far as this controller saw.
    pub since: Option<Timestamp>,
    /// While the internet is down: the furthest hop a traceroute reached.
    #[ts(as = "Option<String>")]
    pub last_hop: Option<IpAddr>,
    /// Why the latest round didn't run or finish (the gateway offline, the probe failing).
    pub error: Option<String>,
    pub round: Option<ProbeRound>,
}

fn view(api: &Api) -> ApiResult<WanMonitorView> {
    let state = api.wan_monitor()?.state();
    Ok(WanMonitorView {
        settings: state.settings.clone(),
        gateway: state.gateway,
        health: state.health,
        since: state.since,
        last_hop: state.last_hop,
        error: state.error.clone(),
        round: state.round.clone(),
    })
}

pub(crate) async fn get(State(api): State<Api>, _: CurrentUser) -> ApiResult<Json<WanMonitorView>> {
    view(&api).map(Json)
}

pub(crate) async fn put(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    body: Result<Json<WanMonitorSettings>, JsonRejection>,
) -> ApiResult<Json<WanMonitorView>> {
    if user.user.role != Role::Admin {
        return Err(ApiError::Forbidden(
            "Only admins change the WAN monitor".to_owned(),
        ));
    }
    let Json(mut settings) = body?;
    for target in &mut settings.targets {
        target.name = target.name.trim().to_owned();
    }
    settings.validate().map_err(ApiError::BadRequest)?;
    let monitor = api.wan_monitor()?;
    let value =
        serde_json::to_value(&settings).map_err(|err| ApiError::Internal(err.to_string()))?;
    api.store()
        .put_setting(WanMonitorSettings::KEY, value)
        .await?;
    monitor.poke();
    let targets: Vec<&str> = settings.targets.iter().map(|t| t.id.as_str()).collect();
    let detail = if settings.enabled {
        format!("on, probing {}", targets.join(", "))
    } else {
        "off".to_owned()
    };
    api.store()
        .record(AuditEntry {
            at: api.now(),
            action: AuditAction::WanMonitor,
            user_id: Some(user.user.id),
            username: Some(user.user.username.clone()),
            ip: Some(client.ip.to_string()),
            detail: Some(detail),
        })
        .await?;
    let mut view = view(&api)?;
    view.settings = settings;
    Ok(Json(view))
}

#[cfg(test)]
#[path = "wan_monitor_tests.rs"]
mod tests;
