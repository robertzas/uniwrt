//! The SSH terminal (features/observability.md, Diagnostics toolkit; D-141): an admin's
//! `GET v1/nodes/{id}/terminal?cols=&rows=` upgrades to a WebSocket that carries root's login
//! shell on the node, on a pseudo-terminal of that size (80 × 24 by default, each at most 1,000),
//! over the controller's SSH connection. Any adopted node, observe mode included: what is changed
//! there shows as drift.
//!
//! - **Protocol:** binary messages carry bytes, the browser's keystrokes one way and the
//!   terminal's output the other; text messages carry JSON, the browser's
//!   [`TerminalClientMessage`] (a resize) and the controller's [`TerminalServerMessage`]
//!   (`opened` once the shell runs, `ended` with the exit code or why, then the close). A
//!   browser message is at most 64 KiB (a paste is sent in parts).
//! - **Ends:** when the shell exits, the browser closes the socket or goes silent, nothing
//!   passes either way for [`IDLE`], the session ends (close code 4401), the controller stops
//!   (1001), or the node's connection is lost. Closing the channel hangs the shell up.
//! - **Bounds:** [`PER_NODE`] terminals per node and [`TOTAL`] in all; one more ends at once,
//!   saying so.
//! - **Audit:** opening (the node and size) and closing (how long it was open, and why it
//!   closed) as [`AuditAction::Terminal`]; keystrokes and output are never recorded.
//! - **Security:** the upgrade is a GET, so it checks that an `Origin`, when sent, is ours, as
//!   the WS hub does; the session is checked again every minute.

mod pump;
#[cfg(test)]
mod tests;

use std::sync::{Arc, Mutex, PoisonError};

use axum::{
    extract::{
        Path, Query, State, WebSocketUpgrade,
        rejection::{PathRejection, QueryRejection},
        ws::rejection::WebSocketUpgradeRejection,
    },
    http::HeaderMap,
    response::Response,
    routing::get,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uniwrt_core::NodeId;
use uniwrt_model::user::Role;
use uniwrt_node::PtySize;
use uniwrt_store::NodeRepo as _;

use crate::{Api, ApiError, ApiResult, Client, CurrentUser, auth::session, security};

/// The most terminals open on one node.
pub(crate) const PER_NODE: usize = 3;
/// The most terminals open in all.
pub(crate) const TOTAL: usize = 16;
/// The largest message a browser may send.
const MAX_INPUT: usize = 64 * 1024;

/// The terminal's size: `cols` × `rows`, 80 × 24 by default.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TerminalQuery {
    cols: Option<u16>,
    rows: Option<u16>,
}

/// What the browser sends as text; keystrokes go as binary messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
#[ts(export)]
pub enum TerminalClientMessage {
    /// The terminal has a new size, each within 1–1,000 (another is ignored).
    Resize { cols: u16, rows: u16 },
}

/// What the controller sends as text; the terminal's output goes as binary messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum TerminalServerMessage {
    /// The shell runs: its output follows.
    Opened,
    /// The terminal ended, and the connection closes: the shell's exit code when it exited,
    /// else why it ended (it may never have opened).
    Ended {
        exit_code: Option<u32>,
        reason: Option<String>,
    },
}

pub(crate) fn routes() -> axum::Router<Api> {
    axum::Router::new().route("/v1/nodes/{id}/terminal", get(open).connect(open))
}

async fn open(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    headers: HeaderMap,
    id: Result<Path<i64>, PathRejection>,
    query: Result<Query<TerminalQuery>, QueryRejection>,
    upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> ApiResult<Response> {
    security::check_origin(&headers, Some(&client))?;
    if user.user.role != Role::Admin {
        return Err(ApiError::Forbidden("Only admins open terminals".to_owned()));
    }
    let Path(id) = id.map_err(|_| ApiError::BadRequest("A node ID is a number".to_owned()))?;
    let id = NodeId::new(id);
    let node = api
        .store()
        .node(id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("There is no node {id}")))?;
    let Query(query) = query.map_err(|err| ApiError::BadRequest(err.body_text()))?;
    let size = PtySize::new(
        query.cols.unwrap_or(PtySize::DEFAULT.cols),
        query.rows.unwrap_or(PtySize::DEFAULT.rows),
    )
    .ok_or_else(|| {
        ApiError::BadRequest(format!(
            "A terminal has 1 to {} columns and rows",
            PtySize::MAX
        ))
    })?;
    let upgrade = upgrade.map_err(|rejection| ApiError::BadRequest(rejection.body_text()))?;
    // `CurrentUser` found the session, so the request carries its token.
    let session_sha256 = session::presented(&headers)
        .ok_or_else(|| ApiError::Unauthorized("Log in first".to_owned()))?;
    let opening = pump::Opening {
        node,
        user: user.user,
        ip: client.ip.to_string(),
        size,
        session_sha256,
    };
    Ok(upgrade
        .max_message_size(MAX_INPUT)
        .max_frame_size(MAX_INPUT)
        .on_upgrade(move |socket| pump::run(api, socket, opening)))
}

/// The open terminals' nodes, for the bounds. Clones share them.
#[derive(Debug, Clone, Default)]
pub(crate) struct Terminals(Arc<Mutex<Vec<NodeId>>>);

/// Holds a terminal's place among the open ones.
#[derive(Debug)]
pub(crate) struct Slot {
    open: Terminals,
    node: NodeId,
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut open = self.open.lock();
        if let Some(at) = open.iter().position(|node| *node == self.node) {
            open.swap_remove(at);
        }
    }
}

impl Terminals {
    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<NodeId>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A place for one more terminal on `node`, unless the bounds are reached (why, then).
    pub(crate) fn take(&self, node: NodeId) -> Result<Slot, String> {
        let mut open = self.lock();
        if open.iter().filter(|open| **open == node).count() >= PER_NODE {
            return Err(format!(
                "This device has {PER_NODE} terminals open already: close one first"
            ));
        }
        if open.len() >= TOTAL {
            return Err(format!(
                "{TOTAL} terminals are open already: close one first"
            ));
        }
        open.push(node);
        Ok(Slot {
            open: self.clone(),
            node,
        })
    }
}
