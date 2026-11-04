//! How jobs word their failures: a [`JobError`] code the UI can act on, and a message.

use std::error::Error as _;

use tracing::warn;
use uniwrt_model::job::{JobError, JobErrorCode};

pub(crate) fn connect_error(err: &uniwrt_node::Error) -> JobError {
    use uniwrt_node::Error;
    let code = match err {
        Error::HostKeyMismatch { .. } => JobErrorCode::HostKeyMismatch,
        Error::AuthRejected { .. } => JobErrorCode::AuthRejected,
        Error::Connect { .. } | Error::Ssh { .. } | Error::Timeout { .. } => {
            JobErrorCode::Unreachable
        }
        _ => JobErrorCode::NodeFailed,
    };
    JobError {
        code,
        message: format!("Cannot log in: {}", chain(err)),
    }
}

pub(crate) fn node_error(err: &uniwrt_node::Error, what: &str) -> JobError {
    let code = match err {
        uniwrt_node::Error::Disconnected { .. } => JobErrorCode::Unreachable,
        _ => JobErrorCode::NodeFailed,
    };
    JobError {
        code,
        message: format!("{what}: {}", chain(err)),
    }
}

pub(crate) fn unsupported(message: String) -> JobError {
    JobError {
        code: JobErrorCode::Unsupported,
        message,
    }
}

/// A controller-side failure: the cause goes to the log, not to the job.
pub(crate) fn internal(what: &str, err: &dyn std::fmt::Display) -> JobError {
    warn!("{what}: {err}");
    JobError {
        code: JobErrorCode::Internal,
        message: format!("The controller failed ({what}); its log has the cause"),
    }
}

/// The error and its causes, as one line.
pub(crate) fn chain(err: &uniwrt_node::Error) -> String {
    let mut text = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

/// A failure of the transaction tool, or of reaching it.
pub(crate) fn txn_error(err: &uniwrt_node::Error, what: &str) -> JobError {
    match err {
        uniwrt_node::Error::Txn { code, msg, .. }
            if code == uniwrt_node::txn::code::UNSUPPORTED =>
        {
            unsupported(format!("{what}: {msg}"))
        }
        _ => node_error(err, what),
    }
}
