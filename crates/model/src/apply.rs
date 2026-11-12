//! An apply's plan (features/lifecycle.md, Render → diff → apply): what applying a revision
//! would change on each node, for the review screen, before anything changes.

use serde::{Deserialize, Serialize};
use uniwrt_core::{JobId, NodeId, RevisionId, Timestamp};
use uniwrt_uci::{Diff, Value};

use crate::{
    node::NodeRole,
    site::node::{RawSection, is_secret_option},
};

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
    /// The packages that drifted on the node (features/lifecycle.md, Drift): changed there
    /// after the controller confirmed them, which this apply overwrites where they change.
    pub drift: Vec<String>,
    /// The node's raw overrides this apply writes (those of the packages it manages), in the
    /// site's order; options holding secrets as vault references, never their values.
    pub raw: Vec<RawSection>,
    /// Services whose start at boot changes.
    pub services: Vec<ServiceChange>,
    /// Packages from OpenWrt's feeds the configuration needs and the node lacks (SQM's,
    /// mwan3), installed before its transaction; a rollback leaves them installed, their
    /// services off (D-076).
    #[serde(default)]
    pub installs: Vec<String>,
    /// Where the controller reaches it once applied, when that moves.
    pub address: Option<AddressChange>,
    /// The network this controller sits on moves: the gateway keeps its old address beside the
    /// new one until the controller reaches it from the new subnet, then drops it
    /// (make-before-break, D-081).
    pub renumbering: Option<Renumbering>,
    /// Its WAN changes with other things, so the WAN changes last, in its own transaction: a
    /// failure there rolls back only the WAN (D-081).
    pub wan_last: bool,
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

/// Masks the values of the options that hold secrets (`key`, `password`, …:
/// [`is_secret_option`]) in a package's diff, for review screens.
pub fn mask(diff: &mut Diff) {
    let hide = |value: &mut Option<Value>| match value {
        Some(Value::Single(text)) => MASKED.clone_into(text),
        Some(Value::List(items)) => items.iter_mut().for_each(|item| MASKED.clone_into(item)),
        None => {}
    };
    for section in &mut diff.sections {
        for option in &mut section.options {
            if is_secret_option(&option.name) {
                hide(&mut option.before);
                hide(&mut option.after);
            }
        }
    }
}

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

/// The renumbering of the network the controller sits on, as the gateway sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Renumbering {
    /// The network's id.
    pub network: String,
    /// Its subnet now, as `192.168.1.0/24`.
    pub from: String,
    /// Its subnet once applied.
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

/// What an apply records once it ends: the nodes that hold `revision` now (or part of it), the
/// address of each one the controller found elsewhere, and the managed packages each holds (drift's baseline).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    pub revision: RevisionId,
    pub job: JobId,
    pub nodes: Vec<AppliedNode>,
}

/// One node an apply changed or moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedNode {
    pub node: NodeId,
    /// Whether it holds the revision now. `false` once the apply confirmed only its first
    /// transactions (the WAN's rolled back, say): its address and packages are recorded, and
    /// it stays behind the revision (D-081).
    pub holds: bool,
    /// Where the controller found it, when that moved.
    pub host: Option<String>,
    /// Its managed packages as it holds them now, by package: what drift compares with. The
    /// packages it held before and no longer manages are forgotten.
    pub packages: std::collections::BTreeMap<String, String>,
}

/// One end of a cable between two nodes: a node's Ethernet port.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CableEnd {
    pub node: NodeId,
    pub port: String,
}

/// A cable between two nodes that failed the wired backhaul's probe (features/mesh.md, Wired
/// backhaul; D-097): later applies leave it a trunk while the same two ports face each other.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct BackhaulFailure {
    /// Its ends, in order.
    pub ends: [CableEnd; 2],
    /// The apply that probed it.
    pub job: JobId,
    pub at: Timestamp,
    /// What the probe found, in words.
    pub why: String,
}

impl BackhaulFailure {
    /// Whether it is the cable between `a` and `b`, in either order.
    pub fn is(&self, a: &CableEnd, b: &CableEnd) -> bool {
        (&self.ends[0], &self.ends[1]) == (a, b) || (&self.ends[0], &self.ends[1]) == (b, a)
    }
}
