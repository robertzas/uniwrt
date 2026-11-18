//! Importing a site file (D-130): the document upgraded, its nodes mapped by MAC, its secrets
//! resolved ([`secrets`](super::secrets)), and the preview to review.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uniwrt_core::{MacAddr, NodeId, RevisionId, Timestamp};
use zeroize::Zeroizing;

use super::{
    FileError, FileNode, SiteFile,
    secrets::{placeholder, resolve_secrets},
};
use crate::{
    node::Node,
    site::{
        SecretRef, SiteConfig, check_secret,
        diff::{Change, diff},
        migrate::upgrade,
        validate::{Issue, Severity, validate},
    },
};

/// What importing a file would save, for review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SiteImportPreview {
    /// Names this preview for accepting it.
    pub id: String,
    /// The revision it was compared with, which must still be the latest to accept it.
    pub base: RevisionId,
    pub file: SiteFileInfo,
    /// What accepting it changes in the site's document; secrets only as references.
    pub changes: Vec<Change>,
    /// The imported document's issues: with an error, it can't be accepted.
    pub issues: Vec<Issue>,
    /// The file's nodes and the adopted node each maps onto.
    pub nodes: Vec<NodeMatch>,
    /// The adopted nodes the file doesn't name, which keep their overrides.
    pub kept: Vec<NodeId>,
    /// Each secret the imported document sets, and where its value comes from.
    pub secrets: Vec<SecretImport>,
}

/// Where a file came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SiteFileInfo {
    pub exported_at: Timestamp,
    pub uniwrt: String,
    pub revision: RevisionId,
    /// The document's schema in the file (upgraded when older).
    pub schema: u64,
    /// Whether it holds secrets.
    pub secrets: bool,
    /// Whether they were opened (with the right passphrase).
    pub opened: bool,
}

/// A node of the file, and the adopted node with its MAC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct NodeMatch {
    /// Its id in the file.
    pub file_node: NodeId,
    pub mac: MacAddr,
    pub name: String,
    /// The adopted node it maps onto; none drops its overrides.
    pub node: Option<NodeId>,
}

/// A secret of the imported document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SecretImport {
    /// Its path in the imported document (node ids mapped).
    pub path: String,
    pub source: SecretSource,
}

/// Where an imported secret's value comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SecretSource {
    /// The file's.
    File,
    /// The latest revision's at the same path.
    Kept,
    /// None: the slot is cleared (a value can be given when accepting).
    Missing,
}

/// A file imported: the document to save, the values to seal under the placeholder references
/// it holds, and the preview (its `id` and `base` left for the caller).
#[derive(Debug, Clone)]
pub struct SiteImport {
    pub site: SiteConfig,
    pub values: BTreeMap<SecretRef, Zeroizing<String>>,
    /// The file's values that failed their check (their slots cleared): errors until values
    /// are given for them ([`fill_secret`]).
    pub rejected: Vec<Issue>,
    pub preview: SiteImportPreview,
}

/// The latest revision: its document, and the values of its secrets by path (only those an
/// opened file also sets need be there), so that a value the file shares keeps its reference.
#[derive(Debug, Clone, Copy)]
pub struct Head<'a> {
    pub config: &'a SiteConfig,
    pub values: &'a BTreeMap<String, Zeroizing<String>>,
}

/// Imports `file` onto `head` with the `adopted` nodes; `values` are the file's secrets by
/// path when its blob was opened.
pub fn import(
    file: SiteFile,
    values: Option<BTreeMap<String, Zeroizing<String>>>,
    head: Head<'_>,
    adopted: &[Node],
) -> Result<SiteImport, FileError> {
    let schema = file
        .config
        .get("schema")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let opened = values.is_some();
    let mut site = upgrade(file.config)?;
    let (nodes, map) = match_nodes(&file.nodes, adopted)?;
    // Node ids of the file become the adopted nodes'; the unmatched nodes' entries go.
    site.nodes.retain(|entry| map.contains_key(&entry.node));
    for entry in &mut site.nodes {
        entry.node = map[&entry.node];
    }
    let values: BTreeMap<String, Zeroizing<String>> = values
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(path, value)| Some((local_path(&path, &map)?, value)))
        .collect();
    let mut rejected = Vec::new();
    let (secrets, sealed) = resolve_secrets(&mut site, head, &values, &mut rejected);
    let head = head.config;
    // The adopted nodes the file doesn't name keep their overrides.
    let named: BTreeSet<NodeId> = map.values().copied().collect();
    let kept: Vec<NodeId> = head
        .nodes
        .iter()
        .filter(|entry| !named.contains(&entry.node))
        .map(|entry| entry.node)
        .collect();
    site.nodes.extend(
        head.nodes
            .iter()
            .filter(|entry| kept.contains(&entry.node))
            .cloned(),
    );
    site.nodes.sort_by_key(|entry| entry.node);
    let ids: BTreeSet<NodeId> = adopted.iter().map(|node| node.id).collect();
    let mut issues = rejected.clone();
    issues.extend(validate(&site, &ids));
    let preview = SiteImportPreview {
        id: String::new(),
        base: RevisionId::new(0),
        file: SiteFileInfo {
            exported_at: file.exported_at,
            uniwrt: file.uniwrt,
            revision: file.revision,
            schema,
            secrets: file.secrets.is_some(),
            opened,
        },
        changes: diff(head, &site),
        issues,
        nodes,
        kept,
        secrets,
    };
    Ok(SiteImport {
        site,
        values: sealed,
        rejected,
        preview,
    })
}

/// Sets `value` for the slot at `path` of an import (one the file left missing): checked as a
/// save checks it, under a new placeholder.
pub fn fill_secret(
    import: &mut SiteImport,
    path: &str,
    value: Zeroizing<String>,
) -> Result<(), Issue> {
    let error = |message: &str| Issue {
        path: path.to_owned(),
        severity: Severity::Error,
        message: message.to_owned(),
    };
    check_secret(path, &value).map_err(error)?;
    let placeholder = placeholder(&import.site, &import.values);
    let slot = import
        .site
        .secret_mut(path)
        .ok_or_else(|| error("The document has no secret here"))?;
    *slot = Some(placeholder.clone());
    import.values.insert(placeholder, value);
    import.rejected.retain(|issue| issue.path != path);
    if let Some(secret) = import.preview.secrets.iter_mut().find(|s| s.path == path) {
        secret.source = SecretSource::File;
    } else {
        import.preview.secrets.push(SecretImport {
            path: path.to_owned(),
            source: SecretSource::File,
        });
    }
    Ok(())
}

/// The file's nodes matched to the adopted ones by MAC, and the map of their ids.
fn match_nodes(
    file: &[FileNode],
    adopted: &[Node],
) -> Result<(Vec<NodeMatch>, BTreeMap<NodeId, NodeId>), FileError> {
    let mut ids = BTreeSet::new();
    let mut macs = BTreeSet::new();
    let mut map = BTreeMap::new();
    let mut nodes = Vec::with_capacity(file.len());
    for entry in file {
        if !ids.insert(entry.node) {
            return Err(FileError::DuplicateNode(entry.node));
        }
        if !macs.insert(entry.mac) {
            return Err(FileError::DuplicateMac(entry.mac));
        }
        let node = adopted
            .iter()
            .find(|node| node.mac == entry.mac)
            .map(|node| node.id);
        if let Some(node) = node {
            map.insert(entry.node, node);
        }
        nodes.push(NodeMatch {
            file_node: entry.node,
            mac: entry.mac,
            name: entry.name.clone(),
            node,
        });
    }
    Ok((nodes, map))
}

/// A secret's path in the file as a path in the imported document: a node's with its new id,
/// none for an unmatched node's.
fn local_path(path: &str, map: &BTreeMap<NodeId, NodeId>) -> Option<String> {
    let Some(rest) = path.strip_prefix("nodes/") else {
        return Some(path.to_owned());
    };
    let (id, tail) = rest.split_once('/')?;
    let node = map.get(&NodeId::new(id.parse().ok()?))?;
    Some(format!("nodes/{node}/{tail}"))
}
