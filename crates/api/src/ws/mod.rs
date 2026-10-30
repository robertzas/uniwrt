//! The WS hub, `GET /api/v1/ws` with a session (ARCHITECTURE.md, API; D-041).
//!
//! - **Protocol:** the client sends [`ClientMessage`]s, `{"sub": ["fleet", "node:3",
//!   "logs?node=3&min=warning"]}`, each replacing its topics. The server answers each new topic
//!   with a snapshot (`fleet_snapshot`, `node_snapshot`, `log_snapshot`), then sends its deltas
//!   (`fleet_delta`, `node_delta`, `log_delta`) at most every 100 ms, and `error` for a topic it
//!   refuses. Every message names the topic as the client spelled it. A connection that falls
//!   behind the bus gets every snapshot again.
//! - **Interest:** while a `node:<id>` topic is held, the node's telemetry is fast
//!   (`Fleet::focus`); a `logs` topic follows its log.
//! - **Security:** the upgrade is a GET, which the CSRF layer lets through, so it checks that
//!   an `Origin`, when sent, is ours. The session is checked again every minute; when it ends
//!   the connection closes with code 4401. Client messages are at most 16 KiB.

mod conn;
mod message;
mod subs;
#[cfg(test)]
mod subs_tests;
#[cfg(test)]
mod tests;
mod topic;

use axum::{
    extract::{State, WebSocketUpgrade, ws::rejection::WebSocketUpgradeRejection},
    http::HeaderMap,
    response::Response,
};

pub use message::{ClientMessage, FactsPatch, ServerMessage, TimedEvent, TimedRoam};

use crate::{Api, ApiError, Client, CurrentUser, auth::session, security};

/// The largest message a client may send.
const MAX_CLIENT_MESSAGE: usize = 16 * 1024;

pub(crate) async fn upgrade(
    State(api): State<Api>,
    _: CurrentUser,
    client: Client,
    headers: HeaderMap,
    upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Result<Response, ApiError> {
    security::check_origin(&headers, Some(&client))?;
    let upgrade = upgrade.map_err(|rejection| ApiError::BadRequest(rejection.body_text()))?;
    // `CurrentUser` found the session, so the request carries its token.
    let session_sha256 = session::presented(&headers)
        .ok_or_else(|| ApiError::Unauthorized("Log in first".to_owned()))?;
    Ok(upgrade
        .max_message_size(MAX_CLIENT_MESSAGE)
        .max_frame_size(MAX_CLIENT_MESSAGE)
        .on_upgrade(move |socket| conn::run(api, socket, session_sha256)))
}
