//! `GET /api/v1/topology`, with a session: the fleet's latest topology (features/observability.md,
//! Topology), which the WS topic `topology` keeps current.

use std::sync::Arc;

use axum::{Json, extract::State};
use uniwrt_topo::Topology;

use crate::{Api, CurrentUser};

pub(crate) async fn get(State(api): State<Api>, _: CurrentUser) -> Json<Arc<Topology>> {
    Json(api.fleet().topology())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::http::StatusCode;
    use tokio::time::Instant;

    use crate::testing::{Demo, json};

    #[tokio::test]
    async fn the_topology_is_the_demo_fleet() {
        let demo = Demo::start().await;
        let deadline = Instant::now() + Duration::from_secs(10);
        while demo.engine.topology().edges.is_empty() {
            assert!(Instant::now() < deadline, "no topology in time");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let res = demo.send(demo.get("/v1/topology")).await;
        assert_eq!(res.status(), StatusCode::OK);
        let topology = json(res).await;
        let gateway = format!("node:{}", demo.ids[0]);
        let nodes = topology["nodes"].as_array().unwrap();
        let node = nodes
            .iter()
            .find(|node| node["id"] == gateway.as_str())
            .unwrap();
        assert_eq!(node["kind"], "gateway");
        assert_eq!(node["parent"], "internet");
    }
}
