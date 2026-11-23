//! One terminal's connection: the shell opened in its slot, its bytes relayed both ways until
//! it ends, and the end told to the browser, closed and audited.

use std::time::Duration;

use axum::{
    body::Bytes,
    extract::ws::{CloseFrame, Message, WebSocket},
};
use tokio::time::{self, Instant};
use tracing::{debug, info, warn};
use uniwrt_model::{
    node::Node,
    user::{AuditAction, AuditEntry, User},
};
use uniwrt_node::{ExitStatus, Output, PtySize, Terminal};
use uniwrt_store::AuditRepo as _;

use super::{Slot, TerminalClientMessage, TerminalServerMessage};
use crate::{
    Api,
    auth::session,
    ws::conn::{
        self, CLOSE_GOING_AWAY, CLOSE_SESSION_ENDED, PING_EVERY, SESSION_CHECK_EVERY, SILENCE_LIMIT,
    },
};

/// A terminal through which nothing passes, either way, for this long is closed.
pub(crate) const IDLE: Duration = Duration::from_mins(30);
/// How long the node may take to start the shell (SSH included).
const START_TIMEOUT: Duration = Duration::from_secs(15);
const CLOSE_NORMAL: u16 = 1000;

/// What the upgrade checked: the node, who asks, and the terminal's size.
pub(super) struct Opening {
    pub(super) node: Node,
    pub(super) user: User,
    pub(super) ip: String,
    pub(super) size: PtySize,
    pub(super) session_sha256: [u8; 32],
}

/// Why a terminal ended.
#[derive(Debug)]
enum End {
    Exited(ExitStatus),
    /// The browser closed the socket.
    Left,
    /// The browser stopped answering pings.
    Silent,
    Idle,
    SessionEnded,
    Stopping,
    /// The node's connection failed: how.
    Lost(String),
}

impl End {
    /// Why it closed, for the audit log.
    fn why(&self) -> String {
        match self {
            Self::Exited(ExitStatus::Code(code)) => format!("the shell exited with {code}"),
            Self::Exited(ExitStatus::Signal(signal)) => {
                format!("the shell ended by signal {signal}")
            }
            Self::Exited(_) => "the shell ended".to_owned(),
            Self::Left => "the browser closed it".to_owned(),
            Self::Silent => "the browser went silent".to_owned(),
            Self::Idle => format!("nothing passed for {} minutes", IDLE.as_secs() / 60),
            Self::SessionEnded => "the session ended".to_owned(),
            Self::Stopping => "the controller stopped".to_owned(),
            Self::Lost(err) => format!("the device's connection failed: {err}"),
        }
    }

    /// What the browser is told, and the close code; nothing once it is gone.
    fn told(&self, node: &str) -> Option<(TerminalServerMessage, u16)> {
        let ended = |reason: String| TerminalServerMessage::Ended {
            exit_code: None,
            reason: Some(reason),
        };
        Some(match self {
            Self::Exited(ExitStatus::Code(code)) => (
                TerminalServerMessage::Ended {
                    exit_code: Some(*code),
                    reason: None,
                },
                CLOSE_NORMAL,
            ),
            Self::Exited(ExitStatus::Signal(signal)) => (
                ended(format!("The shell ended by signal {signal}")),
                CLOSE_NORMAL,
            ),
            Self::Exited(_) => (ended("The shell ended".to_owned()), CLOSE_NORMAL),
            Self::Left | Self::Silent => return None,
            Self::Idle => (
                ended(format!(
                    "Closed after {} minutes without activity",
                    IDLE.as_secs() / 60
                )),
                CLOSE_NORMAL,
            ),
            Self::SessionEnded => (
                ended("Your session ended: sign in again".to_owned()),
                CLOSE_SESSION_ENDED,
            ),
            Self::Stopping => (
                ended("The controller is stopping".to_owned()),
                CLOSE_GOING_AWAY,
            ),
            Self::Lost(err) => (
                ended(format!("The connection to {node} failed: {err}")),
                CLOSE_NORMAL,
            ),
        })
    }
}

/// Serves one terminal's connection.
pub(super) async fn run(api: Api, mut socket: WebSocket, opening: Opening) {
    let name = opening.node.name.clone();
    let (mut terminal, slot) = match start(&api, &opening).await {
        Ok(started) => started,
        Err(reason) => {
            debug!(node = %name, %reason, "a terminal didn't open");
            let ended = TerminalServerMessage::Ended {
                exit_code: None,
                reason: Some(reason),
            };
            finish(&mut socket, &ended, CLOSE_NORMAL).await;
            return;
        }
    };
    let opened = Instant::now();
    let PtySize { cols, rows } = opening.size;
    audit(
        &api,
        &opening,
        format!("terminal on {name} opened ({cols} × {rows})"),
    )
    .await;
    info!(node = %name, user = %opening.user.username, "a terminal opened");
    let end = if send_json(&mut socket, &TerminalServerMessage::Opened)
        .await
        .is_ok()
    {
        pump(&api, &mut socket, &mut terminal, opening.session_sha256).await
    } else {
        End::Left
    };
    // Closes the channel: the node hangs the shell up.
    drop(terminal);
    drop(slot);
    let took = took(opened.elapsed());
    let why = end.why();
    audit(
        &api,
        &opening,
        format!("terminal on {name} closed after {took}: {why}"),
    )
    .await;
    info!(node = %name, user = %opening.user.username, %took, %why, "a terminal closed");
    if let Some((message, code)) = end.told(&name) {
        finish(&mut socket, &message, code).await;
    }
}

/// The shell, in a slot of its own; why not, for the browser.
async fn start(api: &Api, opening: &Opening) -> Result<(Terminal, Slot), String> {
    let node = &opening.node;
    let slot = api.terminals().take(node.id)?;
    let terminal = time::timeout(START_TIMEOUT, api.fleet().terminal(node.id, opening.size))
        .await
        .map_err(|_| {
            format!(
                "{} didn't start a shell within {} s",
                node.name,
                START_TIMEOUT.as_secs()
            )
        })?
        .map_err(|err| match err {
            uniwrt_node::Error::Disconnected { .. } => {
                format!("{} isn't connected: check that it is online", node.name)
            }
            err => format!("{} didn't start a shell: {err}", node.name),
        })?;
    Ok((terminal, slot))
}

/// Relays bytes both ways until the terminal ends.
async fn pump(
    api: &Api,
    socket: &mut WebSocket,
    terminal: &mut Terminal,
    session_sha256: [u8; 32],
) -> End {
    let mut closing = api.closing();
    // Set once and never unset, so a change means the server is stopping.
    if *closing.borrow_and_update() {
        return End::Stopping;
    }
    let mut ping = conn::interval(PING_EVERY);
    let mut check = conn::interval(SESSION_CHECK_EVERY);
    let mut heard = Instant::now();
    let idle = time::sleep(IDLE);
    tokio::pin!(idle);
    loop {
        tokio::select! {
            received = socket.recv() => {
                let Some(Ok(message)) = received else { return End::Left };
                heard = Instant::now();
                match message {
                    Message::Binary(data) => {
                        idle.as_mut().reset(Instant::now() + IDLE);
                        if let Err(err) = terminal.write(data).await {
                            return End::Lost(err.to_string());
                        }
                    }
                    Message::Text(text) => match serde_json::from_str(&text) {
                        Ok(TerminalClientMessage::Resize { cols, rows }) => {
                            if let Some(size) = PtySize::new(cols, rows) {
                                terminal.resize(size);
                            }
                        }
                        Err(err) => debug!(%err, "a terminal's message was not understood"),
                    },
                    Message::Close(_) => return End::Left,
                    Message::Ping(_) | Message::Pong(_) => {}
                }
            }
            output = terminal.next() => match output {
                Some(Ok(Output::Stdout(data) | Output::Stderr(data))) => {
                    idle.as_mut().reset(Instant::now() + IDLE);
                    if conn::send(socket, Message::Binary(data)).await.is_err() {
                        return End::Left;
                    }
                }
                Some(Ok(Output::Exit(status))) => return End::Exited(status),
                Some(Err(err)) => return End::Lost(err.to_string()),
                None => return End::Exited(ExitStatus::Unknown),
            },
            _ = ping.tick() => {
                if heard.elapsed() > SILENCE_LIMIT {
                    return End::Silent;
                }
                if conn::send(socket, Message::Ping(Bytes::new())).await.is_err() {
                    return End::Left;
                }
            }
            _ = check.tick() => match session::current(api, session_sha256).await {
                Ok(Some(_)) => {}
                Ok(None) => return End::SessionEnded,
                Err(err) => warn!(%err, "cannot check a terminal's session"),
            },
            () = &mut idle => return End::Idle,
            _ = closing.changed() => return End::Stopping,
        }
    }
}

async fn send_json(socket: &mut WebSocket, message: &TerminalServerMessage) -> Result<(), ()> {
    let text = serde_json::to_string(message).map_err(|err| {
        warn!(%err, "cannot encode a terminal's message");
    })?;
    conn::send(socket, Message::Text(text.into())).await
}

/// Tells the browser how the terminal ended, and closes.
async fn finish(socket: &mut WebSocket, message: &TerminalServerMessage, code: u16) {
    if send_json(socket, message).await.is_ok() {
        let frame = CloseFrame {
            code,
            reason: "".into(),
        };
        let _ = conn::send(socket, Message::Close(Some(frame))).await;
    }
}

async fn audit(api: &Api, opening: &Opening, detail: String) {
    let entry = AuditEntry {
        at: api.now(),
        action: AuditAction::Terminal,
        user_id: Some(opening.user.id),
        username: Some(opening.user.username.clone()),
        ip: Some(opening.ip.clone()),
        detail: Some(detail),
    };
    if let Err(err) = api.store().record(entry).await {
        warn!(%err, "cannot audit a terminal");
    }
}

/// `12 s`, `4 min 12 s` or `1 h 3 min`.
fn took(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    match secs {
        0..60 => format!("{secs} s"),
        60..3600 => format!("{} min {} s", secs / 60, secs % 60),
        _ => format!("{} h {} min", secs / 3600, secs % 3600 / 60),
    }
}

#[cfg(test)]
mod unit {
    use std::time::Duration;

    use super::took;

    #[test]
    fn durations_read_as_people_say_them() {
        assert_eq!(took(Duration::from_millis(400)), "0 s");
        assert_eq!(took(Duration::from_secs(252)), "4 min 12 s");
        assert_eq!(took(Duration::from_secs(3_790)), "1 h 3 min");
    }
}
