//! A brownfield import's preview (features/lifecycle.md, Brownfield import; D-072, D-073): what
//! accepting the configuration the adopted nodes hold would change in the site, and per node
//! how well the model holds it, for the review screen, before anything is saved.

use serde::{Deserialize, Serialize};
use uniwrt_core::{NodeId, RevisionId};
use uniwrt_uci::Diff;

use crate::{
    apply::PlanIssue,
    node::{NodeMode, NodeRole},
    site::{diff::Change, validate::Issue},
};

/// The preview of importing the nodes into the site's latest revision, `base`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ImportPreview {
    /// Names this preview for accepting it.
    pub id: String,
    pub base: RevisionId,
    /// What accepting it changes in the site's document; secrets only as references.
    pub changes: Vec<Change>,
    /// The imported document's issues: with an error, it can't be accepted.
    pub issues: Vec<Issue>,
    /// Every adopted node, the gateway first.
    pub nodes: Vec<NodeImportView>,
}

impl ImportPreview {
    /// Whether accepting it saves anything.
    pub fn changes(&self) -> bool {
        !self.changes.is_empty()
    }
}

/// One node's part of an import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct NodeImportView {
    pub node: NodeId,
    pub name: String,
    pub role: NodeRole,
    pub mode: NodeMode,
    pub state: NodeImportState,
    /// Why it couldn't be read.
    pub reason: Option<String>,
    /// The share of its options the model holds, 0–100; the rest is kept raw.
    pub confidence: u8,
    /// Each of its sections in the packages render writes, and what became of it.
    pub sections: Vec<ImportedSection>,
    /// What applying the import would still change on it (render's own choices), per package;
    /// options holding secrets read [`MASKED`](crate::apply::MASKED).
    pub residual: Vec<Diff>,
    /// Why it can't render with the import.
    pub issues: Vec<PlanIssue>,
    pub warnings: Vec<PlanIssue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum NodeImportState {
    /// Read and imported.
    Imported,
    /// Read, but it can't render with the import: see its issues.
    Failed,
    /// Not reached or not read: it keeps its settings in the site (see its reason).
    Unread,
}

/// One section of a node and what became of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ImportedSection {
    pub package: String,
    #[serde(rename = "type")]
    pub kind: String,
    /// Its name on the node; `None` for an anonymous section.
    pub name: Option<String>,
    /// The name render gives it, when that differs.
    pub renamed: Option<String>,
    pub fate: SectionFate,
    /// With [`SectionFate::Mapped`], its options render doesn't write, kept as a raw merge.
    pub kept: Vec<String>,
}

/// What became of a section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SectionFate {
    /// The model holds it (some options may be kept raw).
    Mapped,
    /// Kept whole as a raw section.
    Raw,
    /// Dropped by render's design (the firewall's zones and rules on an access point).
    Dropped,
}
