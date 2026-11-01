//! The logs routes, with a session (features/observability.md, Logs):
//!
//! - `GET /api/v1/logs?<filter>&before=<time>:<id>&limit=<n>`: stored entries, newest first, a
//!   page at a time ([`LogPage`]; `next` is the cursor of the following page).
//! - `GET /api/v1/logs/histogram?<filter>&buckets=<n>`: how many entries each span of the range
//!   holds, by severity ([`LogHistogram`]; the last hour by default).
//! - `GET /api/v1/logs/{id}/context?around=<n>`: the entry with those logged around it, from
//!   every source ([`LogContext`]).
//! - `GET /api/v1/logs/sources`: the hosts and programs that logged lately ([`LogSources`]).
//! - `GET /api/v1/logs/patterns`: what the annotations mean ([`LogPatterns`]).
//! - `GET`, `POST /api/v1/logs/searches` and `DELETE /api/v1/logs/searches/{id}`: saved
//!   searches, shared by all users ([`SavedSearches`], [`NewSearch`]).
//!
//! A filter is `nodes=<id,…>&hosts=<name,…>&programs=<name,…>&min=<severity>&q=<text>&
//! annotated=true&from=<ms>&to=<ms>`, every part optional ([`LogFilter`]).

use std::time::Duration;

use axum::{
    Json,
    extract::{Path, Query, State, rejection::QueryRejection},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uniwrt_core::{LogId, NodeId, SearchId, Timestamp};
use uniwrt_model::log::{
    LogBucket, LogFilter, LogPattern, LogRecord, LogSources, SavedSearch, Severity,
};
use uniwrt_store::{Error as StoreError, LogRepo as _};
use uniwrt_syslog::Patterns;

use crate::{Api, ApiError, ApiResult, CurrentUser};

const DEFAULT_LIMIT: usize = 200;
const MAX_LIMIT: usize = 1_000;
const MAX_VALUES: usize = 32;
const MAX_TEXT: usize = 256;
const MAX_AROUND: usize = 200;
const DEFAULT_BUCKETS: usize = 60;
const MAX_BUCKETS: usize = 500;
const DEFAULT_SPAN: Duration = Duration::from_hours(1);
/// How far back the sources go: the default retention.
const SOURCES_SPAN: Duration = Duration::from_hours(14 * 24);
const MAX_NAME: usize = 64;

/// One page of entries, newest first.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct LogPage {
    pub records: Vec<LogRecord>,
    /// The `before` of the next page, when there may be one.
    pub next: Option<String>,
}

/// Entries per span of time, by severity.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct LogHistogram {
    pub from: Timestamp,
    pub to: Timestamp,
    pub step_ms: u64,
    pub buckets: Vec<LogBucket>,
}

/// An entry and those logged around it, oldest first.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct LogContext {
    pub records: Vec<LogRecord>,
}

/// The hosts and programs that logged in the last 14 days.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct LogSourceList {
    pub sources: LogSources,
}

/// What each annotation means, by the `pattern` id entries carry.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct LogPatterns {
    pub patterns: Vec<LogPattern>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SavedSearches {
    pub searches: Vec<SavedSearch>,
}

/// A search to save.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct NewSearch {
    pub name: String,
    pub filter: LogFilter,
}

/// A filter, and a page's or a histogram's parameters (not flattened: query strings carry
/// only text, which `serde(flatten)` cannot turn into numbers).
#[derive(Debug, Default, Deserialize)]
pub(crate) struct LogsQuery {
    nodes: Option<String>,
    hosts: Option<String>,
    programs: Option<String>,
    min: Option<String>,
    q: Option<String>,
    annotated: Option<bool>,
    from: Option<i64>,
    to: Option<i64>,
    before: Option<String>,
    limit: Option<usize>,
    buckets: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ContextQuery {
    around: Option<usize>,
}

pub(crate) async fn list(
    State(api): State<Api>,
    _: CurrentUser,
    query: Result<Query<LogsQuery>, QueryRejection>,
) -> ApiResult<Json<LogPage>> {
    let Query(query) = query.map_err(|err| ApiError::BadRequest(err.body_text()))?;
    let filter = filter(&query)?;
    let before = query.before.as_deref().map(cursor).transpose()?;
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT);
    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(ApiError::BadRequest(format!(
            "`limit` is from 1 to {MAX_LIMIT}"
        )));
    }
    let records = api.store().logs(filter, before, limit).await?;
    let next = (records.len() == limit)
        .then(|| records.last())
        .flatten()
        .map(|last| format!("{}:{}", last.entry.time.as_unix_ms(), last.id));
    Ok(Json(LogPage { records, next }))
}

pub(crate) async fn histogram(
    State(api): State<Api>,
    _: CurrentUser,
    query: Result<Query<LogsQuery>, QueryRejection>,
) -> ApiResult<Json<LogHistogram>> {
    let Query(query) = query.map_err(|err| ApiError::BadRequest(err.body_text()))?;
    let mut filter = filter(&query)?;
    let to = filter.to.take().unwrap_or_else(|| api.now());
    let from = filter
        .from
        .take()
        .unwrap_or_else(|| to.saturating_sub(DEFAULT_SPAN));
    if from >= to {
        return Err(ApiError::BadRequest(
            "`from` must come before `to`".to_owned(),
        ));
    }
    let buckets = query.buckets.unwrap_or(DEFAULT_BUCKETS);
    if !(1..=MAX_BUCKETS).contains(&buckets) {
        return Err(ApiError::BadRequest(format!(
            "`buckets` is from 1 to {MAX_BUCKETS}"
        )));
    }
    let span = u64::try_from(to.saturating_duration_since(from).as_millis()).unwrap_or(u64::MAX);
    // Whole seconds, so that buckets line up with the clock.
    let step_ms = span.div_ceil(buckets as u64).div_ceil(1_000).max(1) * 1_000;
    let buckets = api
        .store()
        .log_histogram(filter, from, to, Duration::from_millis(step_ms))
        .await?;
    Ok(Json(LogHistogram {
        from,
        to,
        step_ms,
        buckets,
    }))
}

pub(crate) async fn context(
    State(api): State<Api>,
    _: CurrentUser,
    Path(id): Path<i64>,
    query: Result<Query<ContextQuery>, QueryRejection>,
) -> ApiResult<Json<LogContext>> {
    let Query(query) = query.map_err(|err| ApiError::BadRequest(err.body_text()))?;
    let around = query.around.unwrap_or(20);
    if around > MAX_AROUND {
        return Err(ApiError::BadRequest(format!(
            "`around` is at most {MAX_AROUND}"
        )));
    }
    let records = api.store().log_context(LogId::new(id), around).await?;
    if records.is_empty() {
        return Err(ApiError::NotFound(format!("There is no log entry {id}")));
    }
    Ok(Json(LogContext { records }))
}

pub(crate) async fn sources(
    State(api): State<Api>,
    _: CurrentUser,
) -> ApiResult<Json<LogSourceList>> {
    let since = api.now().saturating_sub(SOURCES_SPAN);
    let sources = api.store().log_sources(since).await?;
    Ok(Json(LogSourceList { sources }))
}

pub(crate) async fn patterns(_: CurrentUser) -> Json<LogPatterns> {
    Json(LogPatterns {
        patterns: Patterns::embedded().all().to_vec(),
    })
}

pub(crate) async fn searches(
    State(api): State<Api>,
    _: CurrentUser,
) -> ApiResult<Json<SavedSearches>> {
    let searches = api.store().saved_searches().await?;
    Ok(Json(SavedSearches { searches }))
}

pub(crate) async fn save_search(
    State(api): State<Api>,
    user: CurrentUser,
    body: Result<Json<NewSearch>, axum::extract::rejection::JsonRejection>,
) -> ApiResult<(StatusCode, Json<SavedSearch>)> {
    let Json(body) = body?;
    let name = body.name.trim().to_owned();
    if name.is_empty() || name.chars().count() > MAX_NAME || name.chars().any(char::is_control) {
        return Err(ApiError::BadRequest(format!(
            "A search's name is 1 to {MAX_NAME} characters, without control characters"
        )));
    }
    check(&body.filter)?;
    match api
        .store()
        .save_search(name.clone(), body.filter, Some(user.user.id))
        .await
    {
        Ok(saved) => Ok((StatusCode::CREATED, Json(saved))),
        Err(StoreError::Conflict { .. }) => Err(ApiError::Conflict(format!(
            "A search named {name:?} is already saved"
        ))),
        Err(err) => Err(err.into()),
    }
}

pub(crate) async fn delete_search(
    State(api): State<Api>,
    _: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    if api.store().delete_search(SearchId::new(id)).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound(format!("There is no saved search {id}")))
    }
}

fn filter(query: &LogsQuery) -> ApiResult<LogFilter> {
    let nodes = list_of(query.nodes.as_deref())
        .into_iter()
        .map(|id| {
            id.parse::<i64>()
                .ok()
                .filter(|id| *id > 0)
                .map(NodeId::new)
                .ok_or_else(|| ApiError::BadRequest(format!("{id:?} is not a node ID")))
        })
        .collect::<ApiResult<_>>()?;
    let min = query
        .min
        .as_deref()
        .map(|name| {
            Severity::parse(name).ok_or_else(|| {
                ApiError::BadRequest(format!(
                    "{name:?} is not a severity: use emergency, alert, critical, error, warning, \
                     notice, info or debug"
                ))
            })
        })
        .transpose()?;
    let filter = LogFilter {
        nodes,
        hosts: list_of(query.hosts.as_deref()),
        programs: list_of(query.programs.as_deref()),
        min,
        text: query.q.clone().filter(|text| !text.trim().is_empty()),
        annotated: query.annotated.unwrap_or(false),
        from: query.from.map(Timestamp::from_unix_ms),
        to: query.to.map(Timestamp::from_unix_ms),
    };
    check(&filter)?;
    Ok(filter)
}

/// Limits on what a filter may hold.
fn check(filter: &LogFilter) -> ApiResult<()> {
    let longest = filter
        .nodes
        .len()
        .max(filter.hosts.len())
        .max(filter.programs.len());
    if longest > MAX_VALUES {
        return Err(ApiError::BadRequest(format!(
            "A filter names at most {MAX_VALUES} nodes, hosts or programs"
        )));
    }
    if filter
        .text
        .as_ref()
        .is_some_and(|text| text.len() > MAX_TEXT)
    {
        return Err(ApiError::BadRequest(format!(
            "The text to find is at most {MAX_TEXT} bytes"
        )));
    }
    Ok(())
}

/// A comma-separated list; empty parts are skipped.
fn list_of(value: Option<&str>) -> Vec<String> {
    value
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

/// `<time>:<id>`, as a page's `next` gives it.
fn cursor(text: &str) -> ApiResult<(Timestamp, LogId)> {
    text.split_once(':')
        .and_then(|(time, id)| Some((time.parse().ok()?, id.parse().ok()?)))
        .map(|(time, id)| (Timestamp::from_unix_ms(time), LogId::new(id)))
        .ok_or_else(|| ApiError::BadRequest(format!("{text:?} is not a page cursor")))
}

#[cfg(test)]
#[path = "logs_tests.rs"]
mod tests;
