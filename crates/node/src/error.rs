//! Errors of the node transport.

use std::{fmt, io, time::Duration};

use crate::{exec::ExitStatus, host_key::HostKey};

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("cannot reach {target}")]
    Connect {
        target: String,
        #[source]
        source: io::Error,
    },
    #[error("SSH with {target} failed")]
    Ssh {
        target: String,
        #[source]
        source: Box<russh::Error>,
    },
    /// The node presented another host key than the pinned one: quarantine it (ARCHITECTURE.md,
    /// Failure modes).
    #[error("{target} presented host key {presented}, but {pinned} is pinned")]
    HostKeyMismatch {
        target: String,
        presented: HostKey,
        pinned: HostKey,
    },
    #[error("{target} rejected {method} authentication as {user}")]
    AuthRejected {
        target: String,
        user: String,
        method: &'static str,
    },
    #[error("{what} timed out after {after:?}")]
    Timeout { what: String, after: Duration },
    #[error("{what}: the connection is down")]
    Disconnected { what: String },
    #[error("{what} queued more than {limit} bytes of output")]
    Overflow { what: String, limit: usize },
    #[error("{what} exited with {status}: {stderr}")]
    Failed {
        what: String,
        status: ExitStatus,
        stderr: String,
    },
    #[error("the agent is not running: {reason}")]
    AgentUnavailable { reason: String },
    /// The agent answered a request with an error.
    #[error("agent op {op} failed: {code}: {msg}")]
    Remote {
        op: String,
        code: String,
        msg: String,
    },
    #[error("agent protocol violation: {0}")]
    Protocol(String),
    #[error("invalid command: {0}")]
    InvalidArgv(&'static str),
    #[error("invalid {what}")]
    Key {
        what: &'static str,
        #[source]
        source: russh::keys::ssh_key::Error,
    },
    #[error("{what} must be an unencrypted ed25519 key, not {found}")]
    KeyType { what: &'static str, found: String },
    #[error("the system random number generator failed")]
    Random,
}

impl Error {
    pub(crate) fn ssh(target: &(impl fmt::Display + ?Sized), source: russh::Error) -> Self {
        Self::Ssh {
            target: target.to_string(),
            source: Box::new(source),
        }
    }
}
