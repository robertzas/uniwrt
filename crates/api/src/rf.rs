//! The RF insight routes, with a session (features/wifi.md, RF insight):
//!
//! - `GET /api/v1/rf`: the report, each radio's scanning and the settings ([`RfView`]).
//! - `POST /api/v1/rf/scans` (admins): queues scans of every radio, or of a node's (`node`), or of
//!   one of them (`node` and `radio`); answers 202 with the radios queued ([`ScansQueued`]).
//! - `GET /api/v1/rf/scans?node=<id>&radio=<name>&before=<id>&limit=<n>`: stored scans, newest
//!   first, without what they heard ([`ScanSummary`]).
//! - `GET /api/v1/rf/scans/{id}`: one scan with what it heard ([`Scan`]).
//! - `PUT /api/v1/rf/settings` (admins): background scans on or off, their interval (1 to 24
//!   hours) and the planner's DFS policy ([`RfSettings`]).
//! - `GET /api/v1/rf/plan`: the RF planner's plan for the site as it is: per band, each radio's
//!   channel, width and power before and after, its cost term by term and the reasons
//!   ([`Plan`]; features/wifi.md, RF planner). A preview: nothing is applied.

use axum::{
    Json,
    extract::{Path, Query, State, rejection::JsonRejection, rejection::QueryRejection},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uniwrt_core::{NodeId, ScanId};
use uniwrt_engine::RfError;
use uniwrt_model::{
    rf::{DfsPolicy, RadioScan, RfSettings, Scan, ScanSummary},
    user::AuditAction,
};
use uniwrt_rf::{RfView, plan::Plan};
use uniwrt_store::{NodeRepo as _, RevisionRepo as _, RfRepo as _};

use crate::{
    Api, ApiError, ApiResult, Client, CurrentUser,
    alerts::{admin, audit},
};

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 500;

pub(crate) async fn get(State(api): State<Api>, _: CurrentUser) -> ApiResult<Json<RfView>> {
    Ok(Json(RfView::clone(&api.rf()?.view())))
}

pub(crate) async fn plan(State(api): State<Api>, _: CurrentUser) -> ApiResult<Json<Plan>> {
    let rf = api.rf()?;
    let site = api
        .store()
        .head_revision()
        .await?
        .ok_or_else(crate::site::no_site)?
        .config;
    let modes = api
        .store()
        .nodes()
        .await?
        .into_iter()
        .map(|node| (node.id, node.mode))
        .collect();
    Ok(Json(rf.plan(site, modes).await.map_err(rf_error)?))
}

/// Which radios to scan: all, a node's, or one.
#[derive(Debug, Default, Deserialize, TS)]
#[ts(export)]
pub struct ScanRequest {
    #[ts(optional)]
    pub node: Option<NodeId>,
    #[ts(optional)]
    pub radio: Option<String>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ScansQueued {
    pub queued: Vec<RadioScan>,
}

pub(crate) async fn scan(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    body: Result<Json<ScanRequest>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<ScansQueued>)> {
    admin(&user, "start scans")?;
    let Json(request) = body?;
    if request.radio.is_some() && request.node.is_none() {
        return Err(ApiError::BadRequest(
            "A radio is named within its node: give `node` too".to_owned(),
        ));
    }
    let queued = api
        .rf()?
        .scan(request.node, request.radio.clone())
        .await
        .map_err(rf_error)?;
    let detail = match (request.node, &request.radio) {
        (Some(node), Some(radio)) => format!("scan of node {node}'s {radio}"),
        (Some(node), None) => format!("scans of node {node}'s radios"),
        _ => "scans of every radio".to_owned(),
    };
    audit(&api, &user, &client, AuditAction::Rf, detail).await?;
    Ok((StatusCode::ACCEPTED, Json(ScansQueued { queued })))
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ScansQuery {
    node: Option<NodeId>,
    radio: Option<String>,
    before: Option<ScanId>,
    limit: Option<usize>,
}

pub(crate) async fn scans(
    State(api): State<Api>,
    _: CurrentUser,
    query: Result<Query<ScansQuery>, QueryRejection>,
) -> ApiResult<Json<Vec<ScanSummary>>> {
    let Query(query) = query.map_err(|err| ApiError::BadRequest(err.body_text()))?;
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT);
    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(ApiError::BadRequest(format!(
            "`limit` is from 1 to {MAX_LIMIT}"
        )));
    }
    let scans = api
        .store()
        .scans(query.node, query.radio, query.before, limit)
        .await?;
    Ok(Json(scans))
}

pub(crate) async fn one(
    State(api): State<Api>,
    _: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<Scan>> {
    let id = ScanId::new(id);
    api.store()
        .scan(id)
        .await?
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("There is no scan {id}")))
}

pub(crate) async fn put_settings(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    body: Result<Json<RfSettings>, JsonRejection>,
) -> ApiResult<Json<RfSettings>> {
    admin(&user, "change scanning")?;
    let Json(settings) = body?;
    if !RfSettings::INTERVALS_H.contains(&settings.interval_h) {
        return Err(ApiError::BadRequest(format!(
            "Scans run every {} to {} hours",
            RfSettings::INTERVALS_H.start(),
            RfSettings::INTERVALS_H.end()
        )));
    }
    api.rf()?.set_settings(settings).await.map_err(rf_error)?;
    let scans = if settings.scanning {
        format!("background scans every {} h", settings.interval_h)
    } else {
        "background scans off".to_owned()
    };
    let dfs = match settings.dfs {
        DfsPolicy::Avoid => "no DFS channels",
        DfsPolicy::NonWeather => "DFS channels but weather radar's",
        DfsPolicy::All => "every DFS channel",
    };
    let detail = format!("{scans}; the planner may use {dfs}");
    audit(&api, &user, &client, AuditAction::Rf, detail).await?;
    Ok(Json(settings))
}

fn rf_error(err: RfError) -> ApiError {
    match err {
        RfError::NoRadio => ApiError::NotFound("No radio matches".to_owned()),
        RfError::Stopped => ApiError::Unavailable("RF insight is not running".to_owned()),
        RfError::Store(why) | RfError::Planner(why) => ApiError::Internal(why),
    }
}
