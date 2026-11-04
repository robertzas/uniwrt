//! Revisions: every save of the [`SiteConfig`] is an immutable revision in one linear history
//! (D-065), with who made it, when, why, and what changed from its parent.

use serde::{Deserialize, Serialize};
use uniwrt_core::{RevisionId, Timestamp, UserId};

use super::{SiteConfig, diff::Change};

/// A revision without its document, as history lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RevisionMeta {
    pub id: RevisionId,
    /// The revision it was made from; `None` for the first.
    pub parent: Option<RevisionId>,
    pub kind: RevisionKind,
    /// The revision a restore copied.
    pub restored_from: Option<RevisionId>,
    pub author_id: Option<UserId>,
    /// The author's username then; `None` when the controller made it.
    pub author: Option<String>,
    pub message: String,
    pub created_at: Timestamp,
    /// The schema the document was written with.
    pub schema: u32,
    /// How many changes it made to its parent.
    pub changes: u32,
}

/// How a revision came about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum RevisionKind {
    /// The defaults of a new site.
    Defaults,
    /// A user's save.
    Edit,
    /// A copy of an older revision's document.
    Restore,
}

/// A revision with its document, upgraded to the current schema, and its changes from its
/// parent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Revision {
    #[serde(flatten)]
    pub meta: RevisionMeta,
    pub config: SiteConfig,
    pub diff: Vec<Change>,
}

/// A revision to commit after `base`, which must still be the latest (else a conflict).
#[derive(Debug, Clone)]
pub struct NewRevision {
    /// The revision the document was edited from; `None` only for the first.
    pub base: Option<RevisionId>,
    pub kind: RevisionKind,
    pub restored_from: Option<RevisionId>,
    pub author_id: Option<UserId>,
    pub author: Option<String>,
    pub message: String,
    pub config: SiteConfig,
}
