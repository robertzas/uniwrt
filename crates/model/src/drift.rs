//! Drift (features/lifecycle.md, Drift; D-072, D-074): a managed package that changed on its
//! node after the controller confirmed it, and what the user made of it. The agent reports each
//! package's hash ([`NodeFacts::config`](crate::facts::NodeFacts::config)); a hash that isn't the
//! confirmed one is read and diffed, and only a semantic change is drift.

use serde::{Deserialize, Serialize};
use uniwrt_core::{NodeId, RevisionId, Timestamp};
use uniwrt_uci::Diff;

/// A managed package as the controller confirmed it on a node: the file the node held once an
/// apply of `revision` confirmed it, or found it unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirmed {
    pub node: NodeId,
    pub package: String,
    /// SHA-256 of `text`, lowercase hex.
    pub sha256: String,
    pub text: String,
    pub revision: RevisionId,
    pub at: Timestamp,
}

/// A package that differs from what the controller confirmed, as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriftRecord {
    pub node: NodeId,
    pub package: String,
    /// The file's hash as the node reported it; `None` once the file is gone.
    pub sha256: Option<String>,
    /// The file as the controller read it; `None` once it is gone.
    pub text: Option<String>,
    /// When the controller first saw the package differ.
    pub detected_at: Timestamp,
    /// When it last saw it change.
    pub changed_at: Timestamp,
    /// Accepted as it is until the file changes again.
    pub acknowledged: Option<Acknowledged>,
}

/// Who accepted a drift, and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Acknowledged {
    pub at: Timestamp,
    /// The user's name; `None` for a user since removed.
    pub by: Option<String>,
}

/// A drift as the API shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Drift {
    pub node: NodeId,
    pub package: String,
    /// The revision whose apply confirmed the package.
    pub revision: RevisionId,
    pub detected_at: Timestamp,
    pub changed_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub acknowledged: Option<Acknowledged>,
    /// The file is gone from the node.
    pub removed: bool,
    /// Why the node's file can't be read as UCI; the diff is then empty.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub error: Option<String>,
    /// From what the controller confirmed to what the node holds, secrets masked.
    pub diff: Diff,
}

/// A node's package whose drift isn't watched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct DriftMute {
    pub node: NodeId,
    pub package: String,
    pub at: Timestamp,
    /// The user's name; `None` for a user since removed.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub by: Option<String>,
}

/// Every drift and mute (`GET /api/v1/drift`, the WS topic `config`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct DriftState {
    /// By node, then package.
    pub drifts: Vec<Drift>,
    pub muted: Vec<DriftMute>,
}

impl DriftState {
    pub fn drift(&self, node: NodeId, package: &str) -> Option<&Drift> {
        self.drifts
            .iter()
            .find(|d| d.node == node && d.package == package)
    }

    pub fn is_muted(&self, node: NodeId, package: &str) -> bool {
        self.muted
            .iter()
            .any(|m| m.node == node && m.package == package)
    }
}

/// How a drift ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum DriftEnd {
    /// The file is back to what the controller confirmed.
    Reverted,
    /// An apply confirmed the package anew (overwritten, or found as the import made it).
    Confirmed,
    /// A user accepted it until the file changes again.
    Acknowledged,
    /// A user stopped watching the package.
    Muted,
    /// The node is gone, or the controller no longer manages the package there.
    Unwatched,
}

/// What changed about one drift, for the events it makes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriftChange {
    /// A package began to differ from what the controller confirmed (or differs again after an
    /// acknowledgement).
    Detected {
        node: NodeId,
        package: String,
        revision: RevisionId,
        removed: bool,
    },
    Ended {
        node: NodeId,
        package: String,
        how: DriftEnd,
    },
}
