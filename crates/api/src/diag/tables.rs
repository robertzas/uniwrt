//! A node's tables and optional tools (features/observability.md, Diagnostics toolkit; D-142),
//! read on demand over the engine's link to any adopted node, observe mode included, with a
//! session: `GET v1/nodes/{id}/diag/neighbors` (ARP and NDP), `…/routes` (every table of
//! both families), `…/conntrack` ([`ConntrackTable`], the first 4,096) and `…/tools`
//! ([`OptionalToolStatus`] of tcpdump, iperf3 and ethtool, read from the package database,
//! which keeps the stored inventory current too). 409 while the node is offline, 502 when its
//! command fails.

use std::time::Duration;

use axum::{
    Json,
    extract::{Path, State, rejection::PathRejection},
    routing::get,
};
use uniwrt_core::NodeId;
use uniwrt_model::{
    diag::{
        IpFamily,
        tables::{ConntrackTable, MAX_CONNECTIONS, NeighborEntry, Route},
        tools::{OptionalTool, OptionalToolStatus},
    },
    inventory::Inventory,
    node::Node,
};
use uniwrt_node::{ExecRequest, diag::tables, pkg};
use uniwrt_store::{FactsRepo as _, InventoryRepo as _, NodeRepo as _};

use crate::{Api, ApiError, ApiResult, CurrentUser};

/// How long a table's command may take.
const TIMEOUT: Duration = Duration::from_secs(15);
/// The most output a table's command may print: 4,096 connections of up to ~400 bytes.
const MAX_OUTPUT: usize = 2 << 20;

pub(super) fn routes() -> axum::Router<Api> {
    axum::Router::new()
        .route("/v1/nodes/{id}/diag/neighbors", get(neighbors))
        .route("/v1/nodes/{id}/diag/routes", get(route_table))
        .route("/v1/nodes/{id}/diag/conntrack", get(conntrack))
        .route("/v1/nodes/{id}/diag/tools", get(tools))
}

pub(super) async fn node(api: &Api, id: Result<Path<i64>, PathRejection>) -> ApiResult<Node> {
    let Path(id) = id.map_err(|_| ApiError::BadRequest("A node ID is a number".to_owned()))?;
    let id = NodeId::new(id);
    api.store()
        .node(id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("There is no node {id}")))
}

/// The stdout of `argv` on `node`.
async fn read<S: AsRef<str>>(api: &Api, node: &Node, argv: &[S], what: &str) -> ApiResult<String> {
    run(api, node, argv)
        .await
        .map_err(|err| worded(node, what, err))
}

/// The stdout of `argv` on `node`, or why it has none.
async fn run<S: AsRef<str>>(api: &Api, node: &Node, argv: &[S]) -> uniwrt_node::Result<String> {
    let request = ExecRequest::new(argv.iter().map(AsRef::as_ref))
        .timeout(TIMEOUT)
        .max_output(MAX_OUTPUT);
    let output = api
        .fleet()
        .exec(node.id, request)
        .await
        .and_then(|output| output.check(argv[0].as_ref()))?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn worded(node: &Node, what: &str, err: uniwrt_node::Error) -> ApiError {
    match err {
        uniwrt_node::Error::Disconnected { .. } => {
            ApiError::Conflict(format!("{} is offline", node.name))
        }
        other => ApiError::BadGateway(format!("Cannot read {what}: {other}")),
    }
}

async fn neighbors(
    State(api): State<Api>,
    _user: CurrentUser,
    id: Result<Path<i64>, PathRejection>,
) -> ApiResult<Json<Vec<NeighborEntry>>> {
    let node = node(&api, id).await?;
    let text = read(&api, &node, &tables::NEIGHBORS, "the neighbors").await?;
    Ok(Json(tables::parse_neighbors(&text)))
}

async fn route_table(
    State(api): State<Api>,
    _user: CurrentUser,
    id: Result<Path<i64>, PathRejection>,
) -> ApiResult<Json<Vec<Route>>> {
    let node = node(&api, id).await?;
    let (v4, v6) = (
        tables::routes(IpFamily::Ipv4),
        tables::routes(IpFamily::Ipv6),
    );
    let (v4, v6) = tokio::try_join!(
        read(&api, &node, &v4, "the routes"),
        read(&api, &node, &v6, "the routes"),
    )?;
    let mut routes = tables::parse_routes(&v4, IpFamily::Ipv4);
    routes.extend(tables::parse_routes(&v6, IpFamily::Ipv6));
    Ok(Json(routes))
}

async fn conntrack(
    State(api): State<Api>,
    _user: CurrentUser,
    id: Result<Path<i64>, PathRejection>,
) -> ApiResult<Json<ConntrackTable>> {
    let node = node(&api, id).await?;
    let what = "the tracked connections";
    let count = match run(&api, &node, &tables::CONNTRACK_COUNT).await {
        Ok(count) => count,
        // Without the conntrack module (no firewall), the kernel tracks nothing.
        Err(uniwrt_node::Error::Failed { .. }) => {
            return Ok(Json(ConntrackTable {
                connections: Vec::new(),
                count: 0,
                max: None,
                truncated: false,
                listed: true,
            }));
        }
        Err(err) => return Err(worded(&node, what, err)),
    };
    let (count, max) = tables::parse_count(&count);
    let text = match run(&api, &node, &tables::conntrack()).await {
        Ok(text) => text,
        // A kernel built without `NF_CONNTRACK_PROCFS` counts connections but lists none.
        Err(uniwrt_node::Error::Failed { .. }) => {
            return Ok(Json(ConntrackTable {
                connections: Vec::new(),
                count,
                max,
                truncated: false,
                listed: false,
            }));
        }
        Err(err) => return Err(worded(&node, what, err)),
    };
    let connections = tables::parse_conntrack(&text);
    let read_all = text.lines().count() < MAX_CONNECTIONS;
    Ok(Json(ConntrackTable {
        truncated: !read_all && count as usize > connections.len(),
        count: count.max(u32::try_from(connections.len()).unwrap_or(u32::MAX)),
        max,
        connections,
        listed: true,
    }))
}

async fn tools(
    State(api): State<Api>,
    _user: CurrentUser,
    id: Result<Path<i64>, PathRejection>,
) -> ApiResult<Json<Vec<OptionalToolStatus>>> {
    let node = node(&api, id).await?;
    let live = api
        .fleet()
        .node(node.id)
        .and_then(|view| view.facts)
        .and_then(|facts| facts.capabilities.package_manager);
    let manager = match live {
        Some(manager) => Some(manager),
        None => api
            .store()
            .latest_facts(node.id)
            .await?
            .and_then(|facts| facts.capabilities.package_manager),
    };
    let manager = manager.ok_or_else(|| {
        ApiError::Conflict(format!(
            "{}'s package manager isn't known yet: wait for its facts",
            node.name
        ))
    })?;
    let mut outputs = Vec::new();
    for argv in pkg::inventory(manager) {
        outputs.push(read(&api, &node, &argv, "the installed packages").await?);
    }
    let packages = pkg::parse(manager, &outputs);
    let installed = |name: &str| packages.iter().any(|package| package.name == name);
    let statuses = OptionalTool::ALL
        .map(|tool| tool.status(installed))
        .to_vec();
    api.store()
        .set_inventory(
            node.id,
            Inventory {
                taken_at: api.now(),
                packages,
            },
        )
        .await?;
    Ok(Json(statuses))
}

#[cfg(test)]
mod tests;
