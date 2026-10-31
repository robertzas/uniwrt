//! The topology view's layout: where users pinned its vertices, one layout for every user and
//! browser. `GET /api/v1/topology/layout` lists the pins; `PUT /api/v1/topology/layout/{vertex}`
//! pins one (or moves its pin) and `DELETE` there unpins it; `DELETE /api/v1/topology/layout`
//! unpins them all.

use axum::{
    Json,
    extract::{Path, State, rejection::JsonRejection},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uniwrt_store::{Pin, TopologyRepo as _};

use crate::{Api, ApiError, ApiResult, CurrentUser};

/// More pins than any network needs, which bounds what a client can store.
const MAX_PINS: usize = 4096;
/// Longer than any vertex id (`client:` and a MAC is 24).
const MAX_VERTEX: usize = 128;
/// Far beyond any layout, in the view's world units.
const MAX_COORDINATE: f64 = 1e6;

/// A pinned vertex.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub(crate) struct TopologyPin {
    /// A vertex id, such as `node:3` or `client:<mac>`.
    pub(crate) vertex: String,
    pub(crate) x: f64,
    pub(crate) y: f64,
}

/// Where to pin a vertex, in the view's world coordinates.
#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub(crate) struct PinRequest {
    pub(crate) x: f64,
    pub(crate) y: f64,
}

pub(crate) async fn list(
    State(api): State<Api>,
    _: CurrentUser,
) -> ApiResult<Json<Vec<TopologyPin>>> {
    let pins = api.store().pins().await?;
    Ok(Json(
        pins.into_iter()
            .map(|pin| TopologyPin {
                vertex: pin.vertex,
                x: pin.x,
                y: pin.y,
            })
            .collect(),
    ))
}

pub(crate) async fn pin(
    State(api): State<Api>,
    _: CurrentUser,
    Path(vertex): Path<String>,
    body: Result<Json<PinRequest>, JsonRejection>,
) -> ApiResult<StatusCode> {
    let Json(PinRequest { x, y }) = body?;
    let vertex = valid_vertex(vertex)?;
    if ![x, y]
        .iter()
        .all(|value| value.is_finite() && value.abs() <= MAX_COORDINATE)
    {
        return Err(ApiError::BadRequest(format!(
            "Coordinates are numbers within ±{MAX_COORDINATE}"
        )));
    }
    if !api.store().pin(Pin { vertex, x, y }, MAX_PINS).await? {
        return Err(ApiError::Conflict(format!(
            "{MAX_PINS} vertices are pinned already"
        )));
    }
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn unpin(
    State(api): State<Api>,
    _: CurrentUser,
    Path(vertex): Path<String>,
) -> ApiResult<StatusCode> {
    api.store().unpin(&valid_vertex(vertex)?).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn reset(State(api): State<Api>, _: CurrentUser) -> ApiResult<StatusCode> {
    api.store().unpin_all().await?;
    Ok(StatusCode::NO_CONTENT)
}

fn valid_vertex(vertex: String) -> ApiResult<String> {
    if vertex.is_empty()
        || vertex.len() > MAX_VERTEX
        || !vertex.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(ApiError::BadRequest(
            "A vertex id is up to 128 printable ASCII characters".to_owned(),
        ));
    }
    Ok(vertex)
}

#[cfg(test)]
mod tests {
    use axum::http::{Method, StatusCode};
    use serde_json::json;

    use crate::testing::{Demo, json, request};

    #[tokio::test]
    async fn pins_are_kept_moved_and_removed() {
        let demo = Demo::start().await;
        let cookie = Some(demo.cookie.as_str());
        let put = |vertex: &str, body| {
            request(
                Method::PUT,
                &format!("/v1/topology/layout/{vertex}"),
                Some(&body),
                cookie,
            )
        };
        let res = demo
            .send(put("node:1", json!({ "x": 10.5, "y": -3 })))
            .await;
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        let res = demo
            .send(put("client:02:00:00:00:00:09", json!({ "x": 1, "y": 2 })))
            .await;
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        let res = demo.send(put("node:1", json!({ "x": 4, "y": 5 }))).await;
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        let pins = json(demo.send(demo.get("/v1/topology/layout")).await).await;
        assert_eq!(
            pins,
            json!([
                { "vertex": "client:02:00:00:00:00:09", "x": 1.0, "y": 2.0 },
                { "vertex": "node:1", "x": 4.0, "y": 5.0 },
            ])
        );

        let res = demo.send(put("node:1", json!({ "x": 1e9, "y": 0 }))).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let res = demo.send(put("node:1", json!({ "x": 1 }))).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let long = "x".repeat(129);
        let res = demo.send(put(&long, json!({ "x": 1, "y": 1 }))).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);

        let delete = |uri: &str| request(Method::DELETE, uri, None, cookie);
        let res = demo.send(delete("/v1/topology/layout/node:1")).await;
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        let pins = json(demo.send(demo.get("/v1/topology/layout")).await).await;
        assert_eq!(pins.as_array().unwrap().len(), 1);
        let res = demo.send(delete("/v1/topology/layout")).await;
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        let pins = json(demo.send(demo.get("/v1/topology/layout")).await).await;
        assert_eq!(pins, json!([]));
    }
}
