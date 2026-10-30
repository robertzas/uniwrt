//! One WebSocket connection: its subscriptions fed by the engine's bus and flushed every
//! [`FLUSH_EVERY`], kept alive with pings, and closed when its session ends, the peer goes
//! silent, a send stalls or the controller stops.

use std::time::Duration;

use axum::{
    body::Bytes,
    extract::ws::{CloseFrame, Message, WebSocket},
};
use tokio::{
    sync::broadcast::error::RecvError,
    time::{self, Instant, MissedTickBehavior},
};
use tracing::{debug, warn};

use super::{
    message::{ClientMessage, ServerMessage},
    subs::Subscriptions,
};
use crate::{Api, auth::session};

/// How often pending changes go out: at most 10 deltas per second per topic.
pub(crate) const FLUSH_EVERY: Duration = Duration::from_millis(100);
/// How often the server pings; browsers answer on their own.
const PING_EVERY: Duration = Duration::from_secs(30);
/// A peer heard from for this long is gone.
const SILENCE_LIMIT: Duration = Duration::from_secs(75);
/// How often the session is checked: a logout or expiry closes the connection this soon.
pub(crate) const SESSION_CHECK_EVERY: Duration = Duration::from_mins(1);
/// A send that takes longer means a stalled peer.
const SEND_TIMEOUT: Duration = Duration::from_secs(10);

/// Close codes: the session ended (log in again), and the controller is going away.
pub(crate) const CLOSE_SESSION_ENDED: u16 = 4401;
const CLOSE_GOING_AWAY: u16 = 1001;
const STOPPING: &str = "the controller is stopping";

/// Serves the connection of the session whose token hashes to `session_sha256`.
pub(crate) async fn run(api: Api, mut socket: WebSocket, session_sha256: [u8; 32]) {
    if let Some((code, reason)) = serve(&api, &mut socket, session_sha256).await {
        let frame = CloseFrame {
            code,
            reason: reason.into(),
        };
        let _ = send(&mut socket, Message::Close(Some(frame))).await;
    }
}

/// Serves until the connection ends; returns the close frame to send, if any.
async fn serve(
    api: &Api,
    socket: &mut WebSocket,
    session_sha256: [u8; 32],
) -> Option<(u16, &'static str)> {
    // Subscribed before any snapshot is read, so that nothing falls between.
    let mut bus = api.fleet().subscribe();
    let mut closing = api.closing();
    // Set once and never unset, so a change means the server is stopping.
    if *closing.borrow_and_update() {
        return Some((CLOSE_GOING_AWAY, STOPPING));
    }
    let mut subs = Subscriptions::default();
    let mut flush = interval(FLUSH_EVERY);
    let mut ping = interval(PING_EVERY);
    let mut check = interval(SESSION_CHECK_EVERY);
    let mut heard = Instant::now();
    loop {
        let out = tokio::select! {
            received = socket.recv() => {
                let Some(Ok(message)) = received else { return None };
                heard = Instant::now();
                match message {
                    Message::Text(text) => match serde_json::from_str::<ClientMessage>(&text) {
                        Ok(message) => subs.set(api, message.sub).await,
                        Err(err) => vec![not_understood(&err.to_string())],
                    },
                    Message::Binary(_) => vec![not_understood("binary messages are not used")],
                    Message::Close(_) => return None,
                    Message::Ping(_) | Message::Pong(_) => continue,
                }
            }
            event = bus.recv() => match event {
                Ok(event) => {
                    subs.apply(api, &event).await;
                    continue;
                }
                Err(RecvError::Lagged(missed)) => {
                    debug!(missed, "a WS connection fell behind the bus; sending snapshots");
                    subs.resync(api).await
                }
                Err(RecvError::Closed) => return Some((CLOSE_GOING_AWAY, STOPPING)),
            },
            _ = flush.tick() => subs.flush(api),
            _ = ping.tick() => {
                if heard.elapsed() > SILENCE_LIMIT {
                    debug!("a WS peer went silent; closing");
                    return None;
                }
                if send(socket, Message::Ping(Bytes::new())).await.is_err() {
                    return None;
                }
                continue;
            }
            _ = check.tick() => match session::current(api, session_sha256).await {
                Ok(Some(_)) => continue,
                Ok(None) => return Some((CLOSE_SESSION_ENDED, "the session ended")),
                Err(err) => {
                    warn!(%err, "cannot check a WS connection's session");
                    continue;
                }
            },
            _ = closing.changed() => return Some((CLOSE_GOING_AWAY, STOPPING)),
        };
        if send_all(socket, out).await.is_err() {
            return None;
        }
    }
}

/// An interval whose first tick is one period away, and which skips ticks it missed.
fn interval(period: Duration) -> time::Interval {
    let mut interval = time::interval_at(Instant::now() + period, period);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    interval
}

fn not_understood(detail: &str) -> ServerMessage {
    ServerMessage::Error {
        topic: None,
        message: format!("Expected {{\"sub\": [topics]}}: {detail}"),
    }
}

async fn send_all(socket: &mut WebSocket, messages: Vec<ServerMessage>) -> Result<(), ()> {
    for message in messages {
        let text = serde_json::to_string(&message).map_err(|err| {
            warn!(%err, "cannot encode a WS message");
        })?;
        send(socket, Message::Text(text.into())).await?;
    }
    Ok(())
}

async fn send(socket: &mut WebSocket, message: Message) -> Result<(), ()> {
    match time::timeout(SEND_TIMEOUT, socket.send(message)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(err)) => {
            debug!(%err, "a WS send failed");
            Err(())
        }
        Err(_) => {
            debug!("a WS send stalled; closing");
            Err(())
        }
    }
}
