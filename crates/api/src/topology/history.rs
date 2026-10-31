//! The topology's history, for the replay (features/observability.md, Topology): `GET
//! /api/v1/topology/timeline` says how far back it goes and when it changed, and `GET
//! /api/v1/topology/at/{ms}` is the topology at a time, as the history keeps it.

use axum::{
    Json,
    extract::{Path, State, rejection::PathRejection},
};
use serde::Serialize;
use ts_rs::TS;
use uniwrt_core::Timestamp;
use uniwrt_store::TopologyRepo as _;
use uniwrt_topo::Topology;

use crate::{Api, ApiError, ApiResult, CurrentUser};

/// What the replay can show.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub(crate) struct TopologyTimeline {
    /// The earliest time the history can show, or `null` while it is empty.
    pub(crate) start: Option<Timestamp>,
    /// When the topology changed after that, oldest first.
    pub(crate) changes: Vec<Timestamp>,
}

pub(crate) async fn timeline(
    State(api): State<Api>,
    _: CurrentUser,
) -> ApiResult<Json<TopologyTimeline>> {
    let times = api.store().topology_times().await?;
    let start = times
        .iter()
        .find(|(_, keyframe)| *keyframe)
        .map(|(at, _)| *at);
    let changes = times
        .into_iter()
        .filter(|(at, keyframe)| !keyframe && start.is_some_and(|start| *at > start))
        .map(|(at, _)| at)
        .collect();
    Ok(Json(TopologyTimeline { start, changes }))
}

pub(crate) async fn at(
    State(api): State<Api>,
    _: CurrentUser,
    at: Result<Path<i64>, PathRejection>,
) -> ApiResult<Json<Topology>> {
    let Path(ms) =
        at.map_err(|_| ApiError::BadRequest("A time is in Unix milliseconds".to_owned()))?;
    let at = Timestamp::from_unix_ms(ms);
    let frames = api.store().topology_frames(at).await?;
    uniwrt_engine::replay(&frames)
        .map_err(|err| ApiError::Internal(format!("the topology's history: {err}")))?
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("The topology's history starts after {at}")))
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use uniwrt_core::Timestamp;
    use uniwrt_store::{TopologyFrame, TopologyRepo as _};
    use uniwrt_topo::{Topology, TopologyDelta};

    use crate::testing::{Demo, json};

    #[tokio::test]
    async fn the_timeline_and_a_past_topology_come_from_the_history() {
        let demo = Demo::start().await;
        let timeline = json(demo.send(demo.get("/v1/topology/timeline")).await).await;
        assert_eq!(
            timeline,
            serde_json::json!({ "start": null, "changes": [] })
        );
        let res = demo.send(demo.get("/v1/topology/at/5000")).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);

        let keyframe = Topology {
            at: Timestamp::from_unix_ms(1_000),
            ..Topology::default()
        };
        let delta = TopologyDelta {
            at: Timestamp::from_unix_ms(2_000),
            ..TopologyDelta::default()
        };
        for (at, keyframe, data) in [
            (1_000, true, serde_json::to_string(&keyframe).unwrap()),
            (2_000, false, serde_json::to_string(&delta).unwrap()),
        ] {
            let frame = TopologyFrame {
                at: Timestamp::from_unix_ms(at),
                keyframe,
                data,
            };
            demo.store.record_topology(frame).await.unwrap();
        }
        let timeline = json(demo.send(demo.get("/v1/topology/timeline")).await).await;
        assert_eq!(
            timeline,
            serde_json::json!({ "start": 1000, "changes": [2000] })
        );
        let topology = json(demo.send(demo.get("/v1/topology/at/2500")).await).await;
        assert_eq!(topology["at"], 2000);
        let res = demo.send(demo.get("/v1/topology/at/soon")).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }
}
