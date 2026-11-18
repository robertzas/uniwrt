//! The site as a file (features/lifecycle.md, Backups: Site export/import; D-130), for migration
//! or config-as-code: the latest revision's document with a table of its nodes by MAC, and its
//! secrets, if exported, in one passphrase-encrypted blob.
//!
//! - **File ([`SiteFile`]):** `format` [`FORMAT`], `version` [`VERSION`], when, by which
//!   uniwrt, from which revision; `nodes` names each node the document has overrides for by its
//!   MAC; `config` is the document as stored (its secret slots hold their vault references,
//!   which an import reads only as "set"); `secrets` is `null` or an armored age file of a JSON
//!   object of path → value ([`SiteConfig::secrets`]'s paths), which the API seals and opens.
//! - **Import ([`import`]):** the document is upgraded to this schema (a newer one is
//!   refused); each node of the file maps onto the adopted node with its MAC, and one without a
//!   match is dropped with its overrides; an adopted node the file doesn't name keeps the
//!   latest revision's overrides. Each secret slot the file sets takes the file's value
//!   (checked as a save checks it), else the latest revision's secret at the same path
//!   (*kept*), else nothing (*missing*). The result is the document to save, the values to seal
//!   (under placeholder references), and the [`SiteImportPreview`] to review, with the
//!   document's issues and its changes from the latest revision.

mod import;
mod secrets;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uniwrt_core::{MacAddr, NodeId, RevisionId, Timestamp};

pub use self::import::{
    Head, NodeMatch, SecretImport, SecretSource, SiteFileInfo, SiteImport, SiteImportPreview,
    fill_secret, import,
};
use super::{SecretRef, SiteConfig, migrate::SchemaError};
use crate::node::Node;

/// The `format` of a site file.
pub const FORMAT: &str = "uniwrt-site";
/// The file format's version this uniwrt writes and reads.
pub const VERSION: u32 = 1;

/// A site file, as exported.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SiteFile {
    pub format: String,
    pub version: u32,
    pub exported_at: Timestamp,
    /// The version of the uniwrt that exported it.
    pub uniwrt: String,
    /// The revision exported.
    pub revision: RevisionId,
    /// The nodes the document has overrides for.
    pub nodes: Vec<FileNode>,
    /// The site's document ([`SiteConfig`]) at its schema.
    #[cfg_attr(feature = "ts", ts(type = "unknown"))]
    pub config: Value,
    /// The secrets' values as an armored age file, if exported.
    pub secrets: Option<String>,
}

/// A node of the file: its id in the document, and what identifies it elsewhere.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FileNode {
    pub node: NodeId,
    pub mac: MacAddr,
    pub name: String,
}

/// Why a file can't be imported.
#[derive(Debug, thiserror::Error)]
pub enum FileError {
    #[error("this isn't a uniwrt site export")]
    NotSiteFile,
    #[error("this site export is format version {0}; this uniwrt reads version {VERSION}")]
    TooNew(u64),
    #[error("this site export doesn't read: {0}")]
    Invalid(String),
    #[error("{0}")]
    Schema(#[from] SchemaError),
    #[error("the file names node {0} twice")]
    DuplicateNode(NodeId),
    #[error("the file names two nodes with MAC {0}")]
    DuplicateMac(MacAddr),
}

impl SiteFile {
    /// `config` of revision `revision` as a file: its nodes named by their inventory records
    /// (overrides of a node no longer adopted can't be named, so they are left out).
    pub fn new(
        revision: RevisionId,
        config: &SiteConfig,
        adopted: &[Node],
        exported_at: Timestamp,
        uniwrt: &str,
        secrets: Option<String>,
    ) -> Result<Self, FileError> {
        let mut config = config.clone();
        config
            .nodes
            .retain(|entry| adopted.iter().any(|node| node.id == entry.node));
        let nodes = config
            .nodes
            .iter()
            .filter_map(|entry| adopted.iter().find(|node| node.id == entry.node))
            .map(|node| FileNode {
                node: node.id,
                mac: node.mac,
                name: node.name.clone(),
            })
            .collect();
        Ok(Self {
            format: FORMAT.to_owned(),
            version: VERSION,
            exported_at,
            uniwrt: uniwrt.to_owned(),
            revision,
            nodes,
            config: serde_json::to_value(&config)
                .map_err(|err| FileError::Invalid(err.to_string()))?,
            secrets,
        })
    }

    /// The file `value`, if it is a site file of a version this uniwrt reads.
    pub fn parse(value: Value) -> Result<Self, FileError> {
        if value.get("format").and_then(Value::as_str) != Some(FORMAT) {
            return Err(FileError::NotSiteFile);
        }
        match value.get("version").and_then(Value::as_u64) {
            Some(version) if version > u64::from(VERSION) => {
                return Err(FileError::TooNew(version));
            }
            Some(_) => {}
            None => return Err(FileError::Invalid("it has no format version".to_owned())),
        }
        serde_json::from_value(value).map_err(|err| FileError::Invalid(err.to_string()))
    }
}

/// The paths and references of the secrets `config` holds that its file carries (overrides of
/// nodes no longer adopted are left out, as [`SiteFile::new`] leaves them).
pub fn exported_secrets(config: &SiteConfig, adopted: &[Node]) -> Vec<(String, SecretRef)> {
    config
        .secrets()
        .into_iter()
        .filter(|slot| {
            !slot.path.starts_with("nodes/")
                || adopted
                    .iter()
                    .any(|node| slot.path.starts_with(&format!("nodes/{}/", node.id)))
        })
        .filter_map(|slot| Some((slot.path, slot.value?.clone())))
        .collect()
}

#[cfg(test)]
mod tests;
