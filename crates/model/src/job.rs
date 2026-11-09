//! Jobs: what the controller does on nodes step by step (ARCHITECTURE.md, Job runner), such as
//! adopting one, enabling its management or applying the site's configuration, recorded as
//! history. Upgrades and backups come later.

use serde::{Deserialize, Serialize};
use uniwrt_core::{JobId, NodeId, RevisionId, Timestamp, UserId};

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
    /// The revision an apply applies.
    #[serde(default)]
    pub revision: Option<RevisionId>,
    pub state: JobState,
    /// Every step of the job, in order, the ones still to come as pending.
    pub steps: Vec<Step>,
    /// How far each node got, for a job on several (an apply); empty otherwise.
    #[serde(default)]
    pub nodes: Vec<JobNode>,
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
    /// The node it acts on, when known from the start (not for an adoption).
    pub node: Option<NodeId>,
    pub revision: Option<RevisionId>,
    pub created_by: Option<UserId>,
    pub steps: Vec<StepName>,
    pub nodes: Vec<JobNode>,
}

/// One node of a job on several.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct JobNode {
    pub node: NodeId,
    pub name: String,
    pub state: JobNodeState,
    /// What happened to it last, for people.
    pub detail: Option<String>,
}

/// Where a node of an apply stands (features/lifecycle.md, Render → diff → apply).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum JobNodeState {
    Pending,
    /// It holds the revision's configuration already.
    Unchanged,
    /// Not managed: the apply leaves it alone.
    Skipped,
    /// Its transaction is staged and checked; nothing changed yet.
    Staged,
    /// Its transaction is applied, with the dead-man timer armed.
    Applied,
    /// It passed its health checks.
    Verified,
    Confirmed,
    /// Its transaction rolled back, or never applied: it holds what it had.
    RolledBack,
    /// It failed; the job's error says how.
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    /// Adopting a running OpenWrt node in observe mode (features/adoption.md, Path A).
    Adopt,
    /// Enabling management on a node: installing the transaction tool and the boot guard.
    Manage,
    /// Back to observe mode: removing them.
    Unmanage,
    /// Applying a revision of the site's configuration to the managed nodes (D-012, D-069).
    Apply,
    /// Trying a WAN configuration on the gateway, then rolling it back (features/wan.md,
    /// Connectivity test; D-075).
    WanTest,
    /// Measuring the line from the gateway: its rates, and its latency idle and under load
    /// (features/wan.md, SQM; D-076).
    SpeedTest,
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
    /// Give root a random password, kept in the vault, on a node that had none.
    SetPassword,
    /// Read what the node is and runs: its facts and installed packages.
    Inventory,
    /// Take a configuration backup (`sysupgrade -b`) and keep it on the controller.
    Backup,
    /// Record the node, its pinned host key, inventory and backup.
    Record,
    /// Start watching the node live.
    Watch,
    /// Install the transaction tool and the boot guard (`/etc/uniwrt`, D-069).
    InstallTool,
    /// Remove them, leaving the node's configuration as it is.
    RemoveTool,
    /// Read each node's packages and facts, render the revision, and diff.
    Plan,
    /// Install the feed packages the configuration needs where they are missing (D-076).
    Install,
    /// Stage each node's transaction and check it there.
    Stage,
    /// Apply every transaction, each with its dead-man timer armed.
    Apply,
    /// Check each node's health over a new connection.
    Verify,
    /// Confirm every transaction.
    Confirm,
    /// The WAN port has a link (the connectivity test's steps follow, `uniwrt_isp::Step`).
    Link,
    /// A DHCP lease, a PPP session, the static address or the DS-Lite tunnel.
    Address,
    /// A default route, and the ISP's gateway answers.
    Gateway,
    /// Names resolve.
    Dns,
    /// An HTTPS site answers.
    Https,
    /// IPv6: a delegated prefix or the 6rd tunnel, and an IPv6 host answers.
    Ipv6,
    /// IPv6 by the preset's 6rd, after DHCPv6 delegated no prefix.
    SixRd,
    /// Roll the gateway back to what it held.
    Rollback,
    /// Round trips with the line quiet (the speed test's steps follow, D-076).
    Idle,
    /// The download rate, and round trips while downloading.
    Download,
    /// The upload rate, and round trips while uploading.
    Upload,
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
    /// The plan has issues (render refused a node, or a managed node is unreachable): nothing
    /// changed.
    Refused,
    /// An apply failed before any confirm: every node rolled back (or will at its deadline).
    RolledBack,
    /// Some nodes confirmed and others rolled back: the network is split between revisions.
    PartialApply,
    /// The connectivity test found no internet over the WAN tried (its steps say where).
    NoInternet,
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
