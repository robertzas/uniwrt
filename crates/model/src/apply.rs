//! An apply's plan (features/lifecycle.md, Render → diff → apply): what applying a revision
//! would change on each node, for the review screen, before anything changes.

use serde::{Deserialize, Serialize};
use uniwrt_core::{JobId, NodeId, RevisionId, Timestamp};
use uniwrt_uci::Diff;

use crate::{node::NodeRole, site::node::RawSection};

/// The plan of applying `revision`: its nodes in the order they apply (leaves → gateway),
/// then the ones it leaves alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ApplyPlan {
    pub revision: RevisionId,
    pub nodes: Vec<NodePlan>,
}

impl ApplyPlan {
    /// Whether it can apply: no node is refused.
    pub fn ready(&self) -> bool {
        self.nodes
            .iter()
            .all(|node| node.action != PlanAction::Refused)
    }

    /// Whether applying it changes anything.
    pub fn changes(&self) -> bool {
        self.nodes
            .iter()
            .any(|node| node.action == PlanAction::Change)
    }
}

/// What the apply does to one node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct NodePlan {
    pub node: NodeId,
    pub name: String,
    pub role: NodeRole,
    pub action: PlanAction,
    /// Why it is skipped or refused.
    pub reason: Option<String>,
    /// The packages that change, as the node holds them → as rendered; the values of options
    /// that hold secrets (`key`, `password`) read [`MASKED`].
    pub packages: Vec<Diff>,
    /// The node's raw overrides this apply writes (those of the packages it manages), in the
    /// site's order; options holding secrets as vault references, never their values.
    pub raw: Vec<RawSection>,
    /// Services whose start at boot changes.
    pub services: Vec<ServiceChange>,
    /// Where the controller reaches it once applied, when that moves.
    pub address: Option<AddressChange>,
    /// Hops from the gateway, by wire or mesh, when known: the farthest apply first.
    pub hops: Option<u32>,
    /// The dead-man timer: unconfirmed for this long, it rolls back by itself.
    pub timeout_s: u32,
    pub warnings: Vec<PlanIssue>,
    /// What keeps it from applying (render's errors, an address the controller couldn't follow).
    pub issues: Vec<PlanIssue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum PlanAction {
    /// Its packages or services change.
    Change,
    /// It holds the revision's configuration already.
    Unchanged,
    /// Not managed (observe mode): the apply leaves it alone.
    Skipped,
    /// It can't be planned: see its issues or reason. Nothing applies until it can.
    Refused,
}

/// What an option holding a secret reads in a plan.
pub const MASKED: &str = "••••••••";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ServiceChange {
    pub name: String,
    /// Started at boot (`true`), or stopped and disabled.
    pub enable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct AddressChange {
    pub from: String,
    pub to: String,
}

/// Something render or the planner reports, at a path of the site's document (`wlans/home`)
/// or `node`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PlanIssue {
    pub path: String,
    pub message: String,
}

/// The revision a node holds, as its last apply left it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct AppliedRevision {
    pub node: NodeId,
    pub revision: RevisionId,
    /// The apply that confirmed it, or found the node holding it already.
    pub job: JobId,
    pub at: Timestamp,
}

/// What an apply records once it ends: the nodes that hold `revision` now, and the address of
/// each one the controller found elsewhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    pub revision: RevisionId,
    pub job: JobId,
    pub nodes: Vec<(NodeId, Option<String>)>,
}
