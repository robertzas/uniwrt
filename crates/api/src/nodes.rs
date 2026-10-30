//! The node routes, with a session: `GET /api/v1/nodes` summarizes every adopted node (the WS
//! hub's `fleet` topic carries the same [`NodeSummary`]s), and `GET /api/v1/nodes/{id}` is one
//! node with its status and facts ([`NodeDetail`], the `node:<id>` topic's snapshot).

use std::{collections::BTreeMap, sync::Arc};

use axum::{
    Json,
    extract::{Path, State, rejection::PathRejection},
};
use serde::Serialize;
use ts_rs::TS;
use uniwrt_core::{MacAddr, NodeId, Timestamp};
use uniwrt_engine::NodeView;
use uniwrt_model::{
    facts::{NodeFacts, WifiIfaceMode},
    node::{Node, NodeMode, NodeRole},
    status::{NodeState, NodeStatus, StatusReason},
};
use uniwrt_store::NodeRepo as _;

use crate::{Api, ApiError, ApiResult, CurrentUser};

/// A node as the device list shows it: its record, its status and the headline numbers of its
/// latest facts (`null` until any arrive).
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct NodeSummary {
    pub id: NodeId,
    pub name: String,
    pub role: NodeRole,
    pub mode: NodeMode,
    /// The management address.
    pub host: String,
    pub mac: MacAddr,
    pub status: NodeStatus,
    /// Such as `GL.iNet GL-MT6000`.
    pub model: Option<String>,
    /// The firmware release, such as `24.10.4`.
    pub version: Option<String>,
    pub uptime_s: Option<u64>,
    pub cpu_percent: Option<f32>,
    /// The share of memory in use (not available to new processes).
    pub memory_percent: Option<f32>,
    pub temperature_c: Option<f32>,
    /// Wi-Fi clients on its access points (mesh peers and uplinks excluded).
    pub clients: Option<u32>,
    /// When the facts were collected.
    pub collected_at: Option<Timestamp>,
}

/// One node in full: its record, its status and its latest facts.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct NodeDetail {
    pub node: Node,
    pub status: NodeStatus,
    /// Fresh while the node is online, else the last ones seen; `null` until any arrive.
    pub facts: Option<Arc<NodeFacts>>,
}

impl NodeSummary {
    /// `view` is what the engine knows; `None` for a node it does not watch.
    pub(crate) fn new(node: &Node, view: Option<&NodeView>) -> Self {
        let facts = view.and_then(|view| view.facts.as_deref());
        let memory = facts.map(|facts| facts.system.memory);
        Self {
            id: node.id,
            name: node.name.clone(),
            role: node.role,
            mode: node.mode,
            host: node.host.clone(),
            mac: node.mac,
            status: status(node, view),
            model: facts.map(|facts| facts.board.model.clone()),
            version: facts.map(|facts| facts.board.release.version.clone()),
            uptime_s: facts.map(|facts| facts.system.uptime_s),
            cpu_percent: facts.and_then(|facts| facts.system.cpu_percent),
            #[expect(
                clippy::cast_precision_loss,
                clippy::cast_possible_truncation,
                reason = "a percentage"
            )]
            memory_percent: memory
                .filter(|memory| memory.total_bytes > 0)
                .map(|memory| {
                    let used = memory.total_bytes.saturating_sub(memory.available_bytes);
                    (used as f64 * 100.0 / memory.total_bytes as f64) as f32
                }),
            temperature_c: facts.and_then(|facts| facts.system.temperature_c),
            clients: facts.map(clients),
            collected_at: facts.map(|facts| facts.collected_at),
        }
    }
}

impl NodeDetail {
    pub(crate) fn new(node: Node, view: Option<NodeView>) -> Self {
        Self {
            status: status(&node, view.as_ref()),
            facts: view.and_then(|view| view.facts),
            node,
        }
    }
}

/// The engine's status, or offline and stopped for a node it does not watch (one whose link
/// could not be set up).
fn status(node: &Node, view: Option<&NodeView>) -> NodeStatus {
    view.map_or_else(
        || NodeStatus {
            state: NodeState::Offline,
            since: node.updated_at,
            reasons: vec![StatusReason::Stopped],
        },
        |view| view.status.clone(),
    )
}

/// Stations on the node's access-point interfaces.
fn clients(facts: &NodeFacts) -> u32 {
    let aps: Vec<&str> = facts
        .radios
        .iter()
        .flat_map(|radio| &radio.ifaces)
        .filter(|iface| iface.mode == WifiIfaceMode::Ap)
        .filter_map(|iface| iface.name.as_deref())
        .collect();
    let count = facts
        .stations
        .iter()
        .filter(|station| aps.contains(&station.iface.as_str()))
        .count();
    u32::try_from(count).unwrap_or(u32::MAX)
}

pub(crate) async fn list(
    State(api): State<Api>,
    _: CurrentUser,
) -> ApiResult<Json<Vec<NodeSummary>>> {
    let views: BTreeMap<_, _> = api.fleet().all().into_iter().collect();
    let nodes = api.store().nodes().await?;
    Ok(Json(
        nodes
            .iter()
            .map(|node| NodeSummary::new(node, views.get(&node.id)))
            .collect(),
    ))
}

pub(crate) async fn get(
    State(api): State<Api>,
    _: CurrentUser,
    id: Result<Path<i64>, PathRejection>,
) -> ApiResult<Json<NodeDetail>> {
    let Path(id) = id.map_err(|_| ApiError::BadRequest("A node ID is a number".to_owned()))?;
    let id = NodeId::new(id);
    let node = api
        .store()
        .node(id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("There is no node {id}")))?;
    Ok(Json(NodeDetail::new(node, api.fleet().node(id))))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::{body::Body, extract::Request, http::StatusCode};
    use serde_json::Value;
    use tokio::time::Instant;
    use uniwrt_model::{
        facts::NodeFacts,
        node::{NewNode, NodeRole},
    };
    use uniwrt_store::NodeRepo as _;

    use super::clients;
    use crate::testing::{Demo, json};

    #[test]
    fn clients_are_the_stations_of_access_point_interfaces() {
        // Three clients on phy0-ap0 and phy1-ap0, and a mesh peer on phy1-mesh0.
        let facts: NodeFacts =
            serde_json::from_str(include_str!("../../model/fixtures/facts/gateway.json")).unwrap();
        assert_eq!(facts.stations.len(), 4);
        assert_eq!(clients(&facts), 3);
    }

    #[tokio::test]
    async fn the_list_summarizes_every_adopted_node() {
        let demo = Demo::start().await;
        let anonymous = Request::get("/v1/nodes").body(Body::empty()).unwrap();
        assert_eq!(
            demo.send(anonymous).await.status(),
            StatusCode::UNAUTHORIZED
        );
        // Adopted, but not watched: its link could not be set up.
        let unwatched = demo
            .store
            .create_node(NewNode {
                name: "spare".into(),
                mac: "02:00:00:00:00:99".parse().unwrap(),
                board_name: "glinet,gl-mt3000".into(),
                host: "203.0.113.99".into(),
                port: 22,
                role: NodeRole::Ap,
            })
            .await
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let nodes = loop {
            let res = demo.send(demo.get("/v1/nodes")).await;
            assert_eq!(res.status(), StatusCode::OK);
            let nodes = json(res).await;
            let ready = nodes.as_array().unwrap()[..4]
                .iter()
                .all(|node| node["status"]["state"] == "online" && node["clients"].is_number());
            if ready {
                break nodes;
            }
            assert!(Instant::now() < deadline, "the fleet never came online");
            tokio::time::sleep(Duration::from_millis(50)).await;
        };
        let nodes = nodes.as_array().unwrap();
        assert_eq!(nodes.len(), 5);
        assert_eq!(nodes[0]["role"], "gateway");
        assert_eq!(nodes[0]["mode"], "observe");
        assert_eq!(nodes[0]["model"], "GL.iNet GL-MT6000");
        assert!(nodes[0]["version"].is_string() && nodes[0]["uptime_s"].is_number());
        let memory = nodes[0]["memory_percent"].as_f64().unwrap();
        assert!(memory > 0.0 && memory < 100.0, "{memory}");
        let clients: u64 = nodes
            .iter()
            .filter_map(|node| node["clients"].as_u64())
            .sum();
        assert!((20..=30).contains(&clients), "{clients} clients");
        let spare = &nodes[4];
        assert_eq!(spare["id"], unwatched.id.get());
        assert_eq!(spare["status"]["state"], "offline");
        assert_eq!(spare["status"]["reasons"][0]["kind"], "stopped");
        assert_eq!(spare["model"], Value::Null);
    }

    #[tokio::test]
    async fn the_detail_has_the_record_status_and_facts() {
        let demo = Demo::start().await;
        let gateway = demo.ids[0];
        let deadline = Instant::now() + Duration::from_secs(10);
        while demo
            .engine
            .node(gateway)
            .and_then(|view| view.facts)
            .is_none()
        {
            assert!(Instant::now() < deadline, "no facts in time");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let res = demo.send(demo.get(&format!("/v1/nodes/{gateway}"))).await;
        assert_eq!(res.status(), StatusCode::OK);
        let detail = json(res).await;
        assert_eq!(detail["node"]["id"], gateway.get());
        assert_eq!(detail["node"]["role"], "gateway");
        assert!(detail["status"]["state"].is_string());
        assert_eq!(detail["facts"]["board"]["hostname"], detail["node"]["name"]);
        assert_ne!(detail["facts"]["radios"].as_array().unwrap().len(), 0);

        let res = demo.send(demo.get("/v1/nodes/999")).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        assert_eq!(json(res).await["detail"], "There is no node 999");
        let res = demo.send(demo.get("/v1/nodes/abc")).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }
}
