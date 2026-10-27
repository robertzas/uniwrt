//! The node transport: SSH to OpenWrt nodes (russh), the agent protocol's envelope, and the
//! [`NodeLink`] trait through which the engine talks to a node.
//!
//! - **API:** [`ControllerKey`] (the controller's ed25519 identity, kept in the vault as OpenSSH
//!   text); [`HostKey`] and [`probe`] (trust on first use: read a node's host key so the user can
//!   confirm its fingerprint before it is pinned); [`Session`] (one authenticated connection,
//!   direct or through a jump session over `direct-tcpip`) with [`Session::exec`],
//!   [`Session::stream`] and [`Session::upload`]; [`Agent`] (the RPC multiplexer over a running
//!   agent's stdio, see [`proto`]); [`SshLink`] (a supervised connection that reconnects with
//!   jittered backoff, restarts the agent and implements [`NodeLink`]); [`Error`].
//! - **Invariants:** every connection checks the host key against its pin; a mismatch is
//!   [`Error::HostKeyMismatch`] and quarantines the link ([`LinkState::Quarantined`]), never a
//!   warning. Commands are argv arrays quoted for the remote shell ([`shell::command_line`]),
//!   never interpolated. Each channel is drained by its own task, so a slow consumer cannot
//!   stall the connection: a stream with more unread output than its budget ends with
//!   [`Error::Overflow`]. Operations fail fast while a node is down. No RSA keys and no SSH
//!   certificates (D-032).
//! - **Dependencies:** `uniwrt-core`, `uniwrt-model` (the host-key pin), russh with the ring
//!   provider, tokio, `serde_json` (raw values: answers are parsed once, by whoever wants them),
//!   `zeroize`.
//! - **Tests:** unit tests next to the pure parts (quoting through the local `sh`, backoff, line
//!   framing, the envelope, the multiplexer over an in-memory pipe); `tests/` runs sessions and
//!   links against an in-process russh server on loopback, and `tests/dropbear.rs` (ignored
//!   unless run by `just test-dropbear`) against dropbear in OpenWrt rootfs containers.

mod agent;
mod backoff;
mod error;
pub mod exec;
mod host_key;
mod key;
mod lines;
mod link;
pub mod proto;
pub mod shell;
mod ssh;
mod supervisor;

pub use agent::{Agent, HELLO_TIMEOUT};
pub use error::{Error, Result};
pub use exec::{ExecOutput, ExecRequest, ExecStream, ExitStatus, Output};
pub use host_key::HostKey;
pub use key::{AUTHORIZED_KEY_COMMENT, ControllerKey};
pub use link::{AgentState, LinkState, NodeLink};
pub use proto::{AgentEvent, Hello};
pub use ssh::{Credential, Session, SshConfig, Target, probe};
pub use supervisor::{LinkConfig, RetryConfig, SshLink};
