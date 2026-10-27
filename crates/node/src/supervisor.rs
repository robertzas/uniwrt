//! [`SshLink`]: one node's connection, kept up by a task that reconnects with jittered backoff
//! and restarts the agent, and that serves [`NodeLink`] from whatever session is current.

use std::{
    convert::Infallible,
    error::Error as _,
    fmt,
    sync::{Arc, PoisonError, RwLock},
    time::Duration,
};

use bytes::Bytes;
use serde_json::value::RawValue;
use tokio::{
    sync::{broadcast, watch},
    time::Instant,
};
use tracing::{debug, info, warn};
use uniwrt_core::Clock;

use crate::{
    Agent, Credential, Error, HostKey, Result, Session, SshConfig, Target,
    agent::HELLO_TIMEOUT,
    backoff::{Backoff, random_seed},
    exec::{ExecOutput, ExecRequest, ExecStream},
    link::{AgentState, LinkState, NodeLink},
    proto::AgentEvent,
};

/// Events buffered per link for subscribers that fall behind.
const EVENT_BUFFER: usize = 1024;

/// What a link connects to, and how.
#[derive(Debug, Clone)]
pub struct LinkConfig {
    pub target: Target,
    /// Reach the node through this link's session (`direct-tcpip`), such as an isolated segment
    /// behind an adopted node. The link is down while that one is.
    pub via: Option<SshLink>,
    pub host_key: HostKey,
    pub credential: Credential,
    pub ssh: SshConfig,
    /// The agent's argv, started on every connection. None runs no agent: commands only.
    pub agent: Option<Vec<String>>,
    pub retry: RetryConfig,
}

impl LinkConfig {
    pub fn new(target: Target, host_key: HostKey, credential: Credential) -> Self {
        Self {
            target,
            via: None,
            host_key,
            credential,
            ssh: SshConfig::default(),
            agent: None,
            retry: RetryConfig::default(),
        }
    }
}

/// Reconnect and agent restart timing (ARCHITECTURE.md, Node protocol).
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// The first delay; it doubles up to `max`, with jitter.
    pub min: Duration,
    pub max: Duration,
    /// A connection (or agent) that stayed up this long starts the backoff over; a shorter one
    /// keeps it growing, so a node that drops right after login is not hammered.
    pub stable_after: Duration,
    pub hello_timeout: Duration,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            min: Duration::from_secs(1),
            max: Duration::from_secs(60),
            stable_after: Duration::from_secs(60),
            hello_timeout: HELLO_TIMEOUT,
        }
    }
}

/// A supervised connection to one node; clones share it. Dropping the last clone (or
/// [`shutdown`](Self::shutdown)) disconnects and ends its task.
#[derive(Clone)]
pub struct SshLink(Arc<Handle>);

struct Handle {
    core: Arc<Core>,
    stop: watch::Sender<bool>,
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.stop.send_replace(true);
    }
}

struct Core {
    name: String,
    state: watch::Sender<LinkState>,
    current: RwLock<Current>,
    events: broadcast::Sender<AgentEvent>,
}

#[derive(Default, Clone)]
struct Current {
    session: Option<Session>,
    agent: Option<Agent>,
}

impl Core {
    fn current(&self) -> Current {
        self.current
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn update(&self, update: impl FnOnce(&mut Current)) {
        update(&mut self.current.write().unwrap_or_else(PoisonError::into_inner));
    }

    fn set_state(&self, state: LinkState) {
        self.state.send_replace(state);
    }
}

impl SshLink {
    /// Starts connecting in a new task; watch [`NodeLink::state`] for progress.
    pub fn spawn(config: LinkConfig, clock: Arc<dyn Clock>) -> Self {
        let core = Arc::new(Core {
            name: config.target.to_string(),
            state: watch::channel(LinkState::Connecting { attempt: 1 }).0,
            current: RwLock::default(),
            events: broadcast::channel(EVENT_BUFFER).0,
        });
        let (stop, stopped) = watch::channel(false);
        tokio::spawn(run(Arc::clone(&core), config, clock, stopped));
        Self(Arc::new(Handle { core, stop }))
    }

    /// The current session, while the link is up.
    pub fn session(&self) -> Option<Session> {
        self.0.core.current().session
    }

    /// Disconnects and waits until the link has stopped.
    pub async fn shutdown(&self) {
        self.0.stop.send_replace(true);
        let mut state = self.0.core.state.subscribe();
        let _ = state.wait_for(|state| *state == LinkState::Stopped).await;
    }

    fn session_or_down(&self) -> Result<Session> {
        self.session().ok_or_else(|| Error::Disconnected {
            what: self.0.core.name.clone(),
        })
    }
}

impl fmt::Debug for SshLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SshLink({})", self.0.core.name)
    }
}

impl NodeLink for SshLink {
    async fn call(&self, op: &str, args: &RawValue, timeout: Duration) -> Result<Box<RawValue>> {
        let current = self.0.core.current();
        if let Some(agent) = current.agent {
            return agent.call(op, args, timeout).await;
        }
        if current.session.is_none() {
            return Err(Error::Disconnected {
                what: self.0.core.name.clone(),
            });
        }
        let reason = match &*self.0.core.state.borrow() {
            LinkState::Up {
                agent: AgentState::Failed { error, .. },
                ..
            } => error.clone(),
            LinkState::Up {
                agent: AgentState::None,
                ..
            } => "this link runs no agent".to_owned(),
            _ => "the agent is starting".to_owned(),
        };
        Err(Error::AgentUnavailable { reason })
    }

    async fn exec(&self, request: ExecRequest) -> Result<ExecOutput> {
        self.session_or_down()?.exec(&request).await
    }

    async fn stream(&self, argv: Vec<String>) -> Result<ExecStream> {
        self.session_or_down()?.stream(&argv).await
    }

    async fn upload(&self, path: &str, data: Bytes, mode: u32) -> Result<()> {
        self.session_or_down()?.upload(path, data, mode).await
    }

    fn events(&self) -> broadcast::Receiver<AgentEvent> {
        self.0.core.events.subscribe()
    }

    fn state(&self) -> watch::Receiver<LinkState> {
        self.0.core.state.subscribe()
    }
}

/// The link's task: connect, stay connected, back off, until stopped or quarantined.
async fn run(
    core: Arc<Core>,
    config: LinkConfig,
    clock: Arc<dyn Clock>,
    mut stop: watch::Receiver<bool>,
) {
    let mut backoff = Backoff::new(config.retry.min, config.retry.max, random_seed());
    let mut attempt = 0_u32;
    loop {
        attempt = attempt.saturating_add(1);
        core.set_state(LinkState::Connecting { attempt });
        let connected = tokio::select! {
            connected = connect(&config) => connected,
            () = stopped(&mut stop) => break,
        };
        let error = match connected {
            Ok(session) => {
                info!(link = %core.name, "connected");
                let up = Instant::now();
                let ended = stay_up(&core, &config, &*clock, &session, &mut stop).await;
                core.update(|current| *current = Current::default());
                let Some(reason) = ended else { break };
                if up.elapsed() >= config.retry.stable_after {
                    backoff.reset();
                }
                attempt = 0;
                reason
            }
            Err(Error::HostKeyMismatch {
                presented, pinned, ..
            }) => {
                warn!(link = %core.name, %presented, %pinned, "host key mismatch: quarantined");
                core.set_state(LinkState::Quarantined { presented, pinned });
                stopped(&mut stop).await;
                break;
            }
            Err(err) => chain(&err),
        };
        let delay = backoff.next_delay();
        debug!(link = %core.name, %error, ?delay, "down");
        core.set_state(LinkState::Down {
            error,
            retry_at: clock.now().saturating_add(delay),
        });
        tokio::select! {
            () = tokio::time::sleep(delay) => {}
            () = stopped(&mut stop) => break,
        }
    }
    core.update(|current| *current = Current::default());
    core.set_state(LinkState::Stopped);
}

async fn connect(config: &LinkConfig) -> Result<Session> {
    let via = match &config.via {
        Some(link) => Some(link.session().ok_or_else(|| Error::Disconnected {
            what: format!("jump host {}", link.0.core.name),
        })?),
        None => None,
    };
    Session::connect(
        &config.target,
        via.as_ref(),
        &config.host_key,
        &config.credential,
        &config.ssh,
    )
    .await
}

/// Serves the session until it ends (returning why) or the link stops (returning None),
/// starting the agent and restarting it with its own backoff when it goes away.
async fn stay_up(
    core: &Core,
    config: &LinkConfig,
    clock: &dyn Clock,
    session: &Session,
    stop: &mut watch::Receiver<bool>,
) -> Option<String> {
    let since = clock.now();
    core.update(|current| current.session = Some(session.clone()));
    let up = |agent| LinkState::Up { since, agent };
    // Each wait below also ends when the session does, or when the link is stopped.
    macro_rules! or_end {
        ($wait:expr) => {
            tokio::select! {
                value = $wait => value,
                reason = session.closed() => return Some(reason),
                () = stopped(stop) => {
                    session.close().await;
                    return None;
                }
            }
        };
    }

    let Some(argv) = &config.agent else {
        core.set_state(up(AgentState::None));
        let never: Infallible = or_end!(std::future::pending());
        match never {}
    };
    let mut backoff = Backoff::new(config.retry.min, config.retry.max, random_seed());
    loop {
        core.set_state(up(AgentState::Starting));
        let started = or_end!(start_agent(
            session,
            argv,
            &core.events,
            config.retry.hello_timeout
        ));
        let error = match started {
            Ok(agent) => {
                let running = Instant::now();
                core.update(|current| current.agent = Some(agent.clone()));
                core.set_state(up(AgentState::Running(agent.hello().clone())));
                let error = or_end!(agent.closed());
                core.update(|current| current.agent = None);
                if running.elapsed() >= config.retry.stable_after {
                    backoff.reset();
                }
                error
            }
            Err(err) => chain(&err),
        };
        let delay = backoff.next_delay();
        warn!(link = %core.name, %error, ?delay, "the agent is not running; commands still work");
        core.set_state(up(AgentState::Failed {
            error,
            retry_at: clock.now().saturating_add(delay),
        }));
        or_end!(tokio::time::sleep(delay));
    }
}

async fn start_agent(
    session: &Session,
    argv: &[String],
    events: &broadcast::Sender<AgentEvent>,
    hello_timeout: Duration,
) -> Result<Agent> {
    let stream = session.stream(argv).await?;
    Agent::start(stream, events.clone(), hello_timeout).await
}

/// Resolves once the link is told to stop (or its last handle is gone).
async fn stopped(stop: &mut watch::Receiver<bool>) {
    let _ = stop.wait_for(|stop| *stop).await;
}

/// The error and its causes, as one line for the UI: `cannot reach …: Connection refused`.
fn chain(err: &Error) -> String {
    let mut text = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}
