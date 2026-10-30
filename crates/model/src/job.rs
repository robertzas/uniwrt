//! Jobs: what the controller does on nodes step by step (ARCHITECTURE.md, Job runner), such as
//! adopting one, recorded as history. Applying changes, upgrades and backups come later.

use serde::{Deserialize, Serialize};
use uniwrt_core::{JobId, NodeId, Timestamp, UserId};

/// A job and how far it got.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Job {
    pub id: JobId,
    pub kind: JobKind,
    /// What it acts on, for people: `root@host:port` while adopting.
    pub target: String,
    /// The node it acts on; for an adoption, the node it adopted, once recorded.
    pub node: Option<NodeId>,
    pub state: JobState,
    /// Every step of the job, in order, the ones still to come as pending.
    pub steps: Vec<Step>,
    pub error: Option<JobError>,
    /// Who started it; `None` when the controller did.
    pub created_by: Option<UserId>,
    pub started_at: Timestamp,
    pub finished_at: Option<Timestamp>,
}

/// A job about to start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewJob {
    pub kind: JobKind,
    pub target: String,
    pub created_by: Option<UserId>,
    pub steps: Vec<StepName>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    /// Adopting a running OpenWrt node in observe mode (features/adoption.md, Path A).
    Adopt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Running,
    Succeeded,
    Failed,
}

/// One step of a job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Step {
    pub name: StepName,
    pub state: StepState,
    /// What it found or did, for people, such as the warnings of a check.
    pub detail: Option<String>,
    pub started_at: Option<Timestamp>,
    pub finished_at: Option<Timestamp>,
}

impl Step {
    pub fn pending(name: StepName) -> Self {
        Self {
            name,
            state: StepState::Pending,
            detail: None,
            started_at: None,
            finished_at: None,
        }
    }
}

/// The steps of every kind of job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum StepName {
    /// Log in to the node, presenting the host key the user confirmed.
    Connect,
    /// Check that the controller supports the node: OpenWrt release, memory, storage.
    Check,
    /// Add the controller's key to the node's `authorized_keys`.
    InstallKey,
    /// Log in again with the controller's key.
    VerifyKey,
    /// Read what the node is and runs: its facts and installed packages.
    Inventory,
    /// Take a configuration backup (`sysupgrade -b`) and keep it on the controller.
    Backup,
    /// Record the node, its pinned host key, inventory and backup.
    Record,
    /// Start watching the node live.
    Watch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum StepState {
    Pending,
    Running,
    Done,
    /// Not needed this time, such as installing a key the node already has.
    Skipped,
    Failed,
}

/// Why a job failed: a code the UI can act on, and a message for people.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct JobError {
    pub code: JobErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum JobErrorCode {
    /// The node could not be reached, or the connection broke.
    Unreachable,
    /// The node presented another host key than the one the user confirmed.
    HostKeyMismatch,
    /// The node refused the password or the controller's key.
    AuthRejected,
    /// The node runs something the controller does not support, such as an old release.
    Unsupported,
    /// The node is adopted already.
    AlreadyAdopted,
    /// A command or the agent failed on the node.
    NodeFailed,
    /// The controller stopped while the job ran.
    Interrupted,
    /// Something failed in the controller; its log has the cause.
    Internal,
}

#[cfg(test)]
mod tests {
    use serde_json::{json, to_value};

    use super::{JobErrorCode, StepName, StepState};

    #[test]
    fn serde_names() {
        assert_eq!(
            to_value([StepName::InstallKey, StepName::VerifyKey]).unwrap(),
            json!(["install_key", "verify_key"])
        );
        assert_eq!(to_value(StepState::Skipped).unwrap(), json!("skipped"));
        assert_eq!(
            to_value(JobErrorCode::HostKeyMismatch).unwrap(),
            json!("host_key_mismatch")
        );
    }
}
