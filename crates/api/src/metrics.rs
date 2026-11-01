//! The metrics routes, with a session (features/observability.md, Metrics): `GET
//! /api/v1/metrics?entity=<entity>&metrics=<a,b>&from=<ms>&to=<ms>&width=<points>` reads an
//! entity's series over a time range at the resolution that holds it, reduced with LTTB to
//! about `width` points ([`MetricsResponse`]); `GET /api/v1/metrics/series?under=<entity>`
//! lists the series kept ([`SeriesInfo`]), for every entity or for one and those below it.

use std::time::Duration;

use axum::{
    Json,
    extract::{Query, State, rejection::QueryRejection},
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uniwrt_core::Timestamp;
use uniwrt_metrics::{Retention, downsample};
use uniwrt_model::metric::{Entity, Metric, Resolution, Unit};
use uniwrt_store::MetricsRepo as _;

use crate::{Api, ApiError, ApiResult, CurrentUser};

/// The span asked for when the query names none.
const DEFAULT_SPAN: Duration = Duration::from_hours(1);
const DEFAULT_WIDTH: usize = 800;
const WIDTHS: std::ops::RangeInclusive<usize> = 16..=4_000;
/// The most metrics one query reads.
const MAX_METRICS: usize = 8;

/// One entity's series over a time range.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct MetricsResponse {
    pub entity: Entity,
    pub from: Timestamp,
    pub to: Timestamp,
    /// What the series were read at (their tails may come from finer resolutions).
    pub resolution: Resolution,
    pub series: Vec<MetricSeries>,
}

/// One series in columns, oldest first, as a chart draws it: a row of `null`s breaks the line
/// where the data stops.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct MetricSeries {
    pub metric: Metric,
    pub unit: Unit,
    pub at: Vec<Timestamp>,
    pub avg: Vec<Option<f64>>,
    /// The envelope of the values each point stands for; `null` when it is the point itself
    /// throughout (raw windows shown one by one).
    pub min: Option<Vec<Option<f64>>>,
    pub max: Option<Vec<Option<f64>>>,
}

/// The metrics kept for one entity.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SeriesInfo {
    pub entity: Entity,
    pub metrics: Vec<Metric>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MetricsQuery {
    entity: String,
    metrics: String,
    from: Option<i64>,
    to: Option<i64>,
    width: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SeriesQuery {
    under: Option<String>,
}

pub(crate) async fn get(
    State(api): State<Api>,
    _: CurrentUser,
    query: Result<Query<MetricsQuery>, QueryRejection>,
) -> ApiResult<Json<MetricsResponse>> {
    let Query(query) = query.map_err(|err| ApiError::BadRequest(err.body_text()))?;
    let entity = parse_entity(&query.entity)?;
    let metrics = parse_metrics(&query.metrics)?;
    let now = api.now();
    let retention = Retention::default();
    let to = query.to.map_or(now, Timestamp::from_unix_ms);
    let from = query
        .from
        .map_or_else(|| to.saturating_sub(DEFAULT_SPAN), Timestamp::from_unix_ms);
    if from >= to {
        return Err(ApiError::BadRequest(
            "`from` must come before `to`".to_owned(),
        ));
    }
    if to.saturating_duration_since(from) > retention.hour {
        return Err(ApiError::BadRequest(format!(
            "A query spans at most {} days",
            retention.hour.as_secs() / 86_400
        )));
    }
    let width = query.width.unwrap_or(DEFAULT_WIDTH);
    if !WIDTHS.contains(&width) {
        return Err(ApiError::BadRequest(format!(
            "`width` is from {} to {} points",
            WIDTHS.start(),
            WIDTHS.end()
        )));
    }
    let resolution = retention.resolution(from, to, now);
    let mut series = Vec::with_capacity(metrics.len());
    for metric in metrics {
        let points = api
            .store()
            .metric_points(&entity, metric, resolution, from, to)
            .await?;
        let drawn = downsample(&points, resolution, width);
        let flat = drawn.min == drawn.avg && drawn.max == drawn.avg;
        series.push(MetricSeries {
            metric,
            unit: metric.unit(),
            at: drawn.at,
            min: (!flat).then_some(drawn.min),
            max: (!flat).then_some(drawn.max),
            avg: drawn.avg,
        });
    }
    Ok(Json(MetricsResponse {
        entity,
        from,
        to,
        resolution,
        series,
    }))
}

pub(crate) async fn series(
    State(api): State<Api>,
    _: CurrentUser,
    query: Result<Query<SeriesQuery>, QueryRejection>,
) -> ApiResult<Json<Vec<SeriesInfo>>> {
    let Query(query) = query.map_err(|err| ApiError::BadRequest(err.body_text()))?;
    let under = query.under.as_deref().map(parse_entity).transpose()?;
    let series = api.store().metric_series(under).await?;
    Ok(Json(
        series
            .into_iter()
            .map(|(entity, metrics)| SeriesInfo { entity, metrics })
            .collect(),
    ))
}

fn parse_entity(text: &str) -> ApiResult<Entity> {
    text.parse()
        .map_err(|err: uniwrt_model::metric::ParseEntityError| {
            ApiError::BadRequest(err.to_string())
        })
}

fn parse_metrics(text: &str) -> ApiResult<Vec<Metric>> {
    let mut metrics = Vec::new();
    for name in text.split(',').filter(|name| !name.is_empty()) {
        let metric: Metric =
            name.parse()
                .map_err(|err: uniwrt_model::metric::ParseMetricError| {
                    ApiError::BadRequest(err.to_string())
                })?;
        if !metrics.contains(&metric) {
            metrics.push(metric);
        }
    }
    if metrics.is_empty() || metrics.len() > MAX_METRICS {
        return Err(ApiError::BadRequest(format!(
            "`metrics` names 1 to {MAX_METRICS} metrics, separated by commas"
        )));
    }
    Ok(metrics)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::http::StatusCode;
    use uniwrt_core::{Clock as _, SystemClock, Timestamp};
    use uniwrt_model::metric::{Entity, Metric, Sample};
    use uniwrt_store::MetricsRepo as _;

    use crate::testing::{Demo, json};

    #[tokio::test]
    async fn series_are_read_downsampled_and_listed() {
        let demo = Demo::start_with(false).await;
        let node = Entity::Node(demo.ids[0]);
        let now = SystemClock.now();
        let ago = |windows: u32| now.saturating_sub(Duration::from_secs(u64::from(windows) * 15));
        // Half an hour of windows, a gap of ten minutes, then ten minutes more.
        let mut samples = Vec::new();
        for window in (0..120_u32).filter(|window| !(40..80).contains(window)) {
            samples.push(Sample {
                entity: node.clone(),
                metric: Metric::Cpu,
                at: ago(120 - window),
                value: f64::from(window),
            });
        }
        demo.store.record_metrics(samples).await.unwrap();

        let from = ago(130).as_unix_ms();
        let uri = format!("/v1/metrics?entity={node}&metrics=cpu,memory&from={from}");
        let res = demo.send(demo.get(&uri)).await;
        assert_eq!(res.status(), StatusCode::OK);
        let body = json(res).await;
        assert_eq!(body["resolution"], "raw");
        assert_eq!(body["entity"], node.to_string());
        let cpu = &body["series"][0];
        assert_eq!(cpu["metric"], "cpu");
        assert_eq!(cpu["unit"], "percent");
        let avg = cpu["avg"].as_array().unwrap();
        // 80 windows and the row that breaks the line.
        assert_eq!(avg.len(), 81);
        assert!(avg[40].is_null());
        assert!(cpu["min"].is_null());
        // Memory has no data.
        assert_eq!(body["series"][1]["at"].as_array().unwrap().len(), 0);

        // Downsampled, with envelopes.
        let res = demo.send(demo.get(&format!("{uri}&width=20"))).await;
        let body = json(res).await;
        let cpu = &body["series"][0];
        assert!(cpu["avg"].as_array().unwrap().len() <= 21);
        assert!(cpu["min"].is_array());

        let res = demo
            .send(demo.get(&format!("/v1/metrics/series?under={node}")))
            .await;
        assert_eq!(res.status(), StatusCode::OK);
        let series = json(res).await;
        assert_eq!(series[0]["entity"], node.to_string());
        assert_eq!(series[0]["metrics"], serde_json::json!(["cpu"]));
    }

    #[tokio::test]
    async fn malformed_queries_are_refused() {
        let demo = Demo::start_with(false).await;
        let now = Timestamp::from_unix_secs(1_800_000_000).as_unix_ms();
        for uri in [
            "/v1/metrics?entity=node:1".to_owned(),
            "/v1/metrics?entity=nodes&metrics=cpu".to_owned(),
            "/v1/metrics?entity=node:1&metrics=cpu,heat".to_owned(),
            "/v1/metrics?entity=node:1&metrics=cpu&width=1".to_owned(),
            format!("/v1/metrics?entity=node:1&metrics=cpu&from={now}&to={now}"),
            format!("/v1/metrics?entity=node:1&metrics=cpu&from=0&to={now}"),
            "/v1/metrics/series?under=node:x".to_owned(),
        ] {
            let res = demo.send(demo.get(&uri)).await;
            assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{uri}");
        }
        // A session is needed.
        let res = demo
            .send(
                axum::extract::Request::get("/v1/metrics/series")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }
}
