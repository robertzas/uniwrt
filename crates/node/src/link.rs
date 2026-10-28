//! [`NodeLink`]: how the engine talks to one node, whatever carries it: [`SshLink`](crate::SshLink)
//! here, the simulated fleet in the `sim` crate (M1.5).

use std::time::Duration;

use bytes::Bytes;
use serde_json::value::RawValue;
use tokio::sync::{broadcast, watch};
use uniwrt_core::Timestamp;

use crate::{
    Error, HostKey, Result,
    exec::{ExecOutput, ExecRequest, ExecStream},
    proto::{AgentEvent, Hello, ops::Op},
};

/// One node's connection. Every operation fails fast while the node is unreachable; nothing
/// queues up for later.
pub trait NodeLink: Send + Sync + 'static {
    /// Sends one request to the agent and waits for its answer.
    fn call(
        &self,
        op: &str,
        args: &RawValue,
        timeout: Duration,
    ) -> impl Future<Output = Result<Box<RawValue>>> + Send;

    /// Sends a typed request ([`proto::ops`](crate::proto::ops)) and decodes its answer; an
    /// answer of the wrong shape is [`Error::Protocol`].
    fn request<O: Op>(
        &self,
        op: &O,
        timeout: Duration,
    ) -> impl Future<Output = Result<O::Output>> + Send {
        async move {
            let args = serde_json::value::to_raw_value(op)
                .map_err(|err| Error::Protocol(format!("cannot encode {}: {err}", O::NAME)))?;
            let answer = self.call(O::NAME, &args, timeout).await?;
            serde_json::from_str(answer.get())
                .map_err(|err| Error::Protocol(format!("unexpected answer to {}: {err}", O::NAME)))
        }
    }

    /// Runs a command to completion.
    fn exec(&self, request: ExecRequest) -> impl Future<Output = Result<ExecOutput>> + Send;

    /// Starts a long-running command.
    fn stream(&self, argv: Vec<String>) -> impl Future<Output = Result<ExecStream>> + Send;

    /// Writes a file atomically.
    fn upload(&self, path: &str, data: Bytes, mode: u32)
    -> impl Future<Output = Result<()>> + Send;

    /// The agent's events and ticks, across reconnects. A receiver that falls behind skips
    /// ahead (`RecvError::Lagged`) rather than holding the node back.
    fn events(&self) -> broadcast::Receiver<AgentEvent>;

    /// The connection's state, from which the engine derives the node's status.
    fn state(&self) -> watch::Receiver<LinkState>;
}

/// Where a link stands.
#[derive(Debug, Clone, PartialEq)]
pub enum LinkState {
    /// Connecting: the `attempt`-th time since the link was last up.
    Connecting { attempt: u32 },
    /// Connected and logged in since `since`.
    Up { since: Timestamp, agent: AgentState },
    /// Not connected; the next attempt is at `retry_at`.
    Down { error: String, retry_at: Timestamp },
    /// The node presented another host key than the pinned one. The link stays down until an
    /// admin re-trusts the node, which starts a new link.
    Quarantined { presented: HostKey, pinned: HostKey },
    /// Shut down.
    Stopped,
}

/// The agent's state while a link is up.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentState {
    /// The link runs no agent: commands only.
    None,
    Starting,
    Running(Hello),
    /// The agent is not running; commands still work (degraded mode, ARCHITECTURE.md). It is
    /// started again at `retry_at`.
    Failed {
        error: String,
        retry_at: Timestamp,
    },
}
