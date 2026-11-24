//! The UCI explorer's read (features/observability.md, Diagnostics toolkit › UCI explorer;
//! D-141, D-143), with a session: `GET v1/nodes/{id}/uci` reads every package in the node's
//! `/etc/config` live over the engine's link (any adopted node, observe mode included) and
//! answers a [`NodeUci`]: each package parsed with secret options masked, who writes it, and the
//! node's raw sections of the site's latest revision placed on the sections they find
//! (`uniwrt_render::explorer`). 409 while the node is offline, 502 when the read fails.

use std::time::Duration;

use axum::{
    Json,
    extract::{Path, State, rejection::PathRejection},
};
use uniwrt_model::{explorer::NodeUci, node::NodeMode};
use uniwrt_node::txn::current;
use uniwrt_store::RevisionRepo as _;

use crate::{Api, ApiError, ApiResult, CurrentUser};

/// How long the read may take.
const TIMEOUT: Duration = Duration::from_secs(20);
/// The most it may print: `/etc/config` is tens of KiB on most nodes.
const MAX_OUTPUT: usize = 8 << 20;

pub(crate) async fn get(
    State(api): State<Api>,
    _user: CurrentUser,
    id: Result<Path<i64>, PathRejection>,
) -> ApiResult<Json<NodeUci>> {
    let node = crate::diag::node(&api, id).await?;
    let read = current::all();
    let request = read.request.clone().timeout(TIMEOUT).max_output(MAX_OUTPUT);
    let files = api
        .fleet()
        .exec(node.id, request)
        .await
        .and_then(|output| read.parse(output))
        .map_err(|err| match err {
            uniwrt_node::Error::Disconnected { .. } => {
                ApiError::Conflict(format!("{} is offline", node.name))
            }
            other => ApiError::BadGateway(format!("Cannot read its configuration: {other}")),
        })?;
    let head = api.store().head_revision().await?;
    let config = head
        .as_ref()
        .and_then(|head| head.config.nodes.iter().find(|n| n.node == node.id));
    let tree = uniwrt_render::explorer::tree(&files, config, node.mode == NodeMode::Managed);
    Ok(Json(NodeUci {
        node: node.id,
        read_at: api.now(),
        revision: head.map(|head| head.meta.id),
        packages: tree.packages,
        unmatched: tree.unmatched,
    }))
}

#[cfg(test)]
mod tests;
