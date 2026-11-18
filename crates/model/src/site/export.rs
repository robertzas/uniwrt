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

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uniwrt_core::{MacAddr, NodeId, RevisionId, Timestamp};
use zeroize::Zeroizing;

use super::{
    SecretRef, SiteConfig, check_secret,
    diff::{Change, diff},
    migrate::{SchemaError, upgrade},
    node::RawValue,
    validate::{Issue, Severity, validate},
};
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

/// Points every secret slot the imported document sets at its value: the file's (under a
/// placeholder, returned to seal, unless the latest revision holds it at the same path), the
/// latest revision's at the same path, or none.
fn resolve_secrets(
    site: &mut SiteConfig,
    Head {
        config: head,
        values: current,
    }: Head<'_>,
    values: &BTreeMap<String, Zeroizing<String>>,
    issues: &mut Vec<Issue>,
) -> (Vec<SecretImport>, BTreeMap<SecretRef, Zeroizing<String>>) {
    // First every distinct reference of the file becomes a placeholder of its own, so that the
    // file's references never meet the vault's (nor each other once kept or cleared).
    let mut refs: Vec<(SecretRef, String)> = Vec::new();
    for slot in site.secrets() {
        if let Some(value) = slot.value
            && !refs.iter().any(|(seen, _)| seen == value)
        {
            refs.push((value.clone(), slot.path));
        }
    }
    let taken: BTreeSet<SecretRef> = head
        .secrets()
        .iter()
        .chain(site.secrets().iter())
        .filter_map(|slot| slot.value.cloned())
        .collect();
    let mut next = 1_u128;
    let mut placeholders = Vec::with_capacity(refs.len());
    for (_, path) in &refs {
        while taken.contains(&SecretRef::numbered(next)) {
            next += 1;
        }
        placeholders.push((SecretRef::numbered(next), path.clone()));
        next += 1;
    }
    let renamed: Vec<SecretRef> = placeholders.iter().map(|(p, _)| p.clone()).collect();
    for ((from, _), to) in refs.iter().zip(&renamed) {
        site.replace_secret(from, to);
    }
    let mut sources = Vec::new();
    let mut sealed = BTreeMap::new();
    for (placeholder, path) in placeholders {
        let source = match values.get(&path) {
            Some(value) => match check_secret(&path, value) {
                // The value the latest revision holds there: its reference, so nothing changes.
                Ok(())
                    if let Some(kept) = head
                        .secret(&path)
                        .filter(|_| current.get(&path) == Some(value)) =>
                {
                    site.replace_secret(&placeholder, kept);
                    SecretSource::File
                }
                Ok(()) => {
                    sealed.insert(placeholder.clone(), value.clone());
                    SecretSource::File
                }
                Err(why) => {
                    issues.push(Issue {
                        path: path.clone(),
                        severity: Severity::Error,
                        message: why.to_owned(),
                    });
                    clear_secret(site, &placeholder);
                    SecretSource::Missing
                }
            },
            None => {
                if let Some(kept) = head.secret(&path) {
                    site.replace_secret(&placeholder, kept);
                    SecretSource::Kept
                } else {
                    clear_secret(site, &placeholder);
                    SecretSource::Missing
                }
            }
        };
        sources.push(SecretImport { path, source });
    }
    (sources, sealed)
}

/// A placeholder reference that neither `site` nor `values` holds yet.
fn placeholder(site: &SiteConfig, values: &BTreeMap<SecretRef, Zeroizing<String>>) -> SecretRef {
    let held: BTreeSet<SecretRef> = site
        .secrets()
        .iter()
        .filter_map(|slot| slot.value.cloned())
        .collect();
    // One of the first `held + values + 1` numbers is free.
    let count = u128::try_from(held.len() + values.len()).unwrap_or(u128::MAX - 1);
    (1..=count + 1)
        .map(SecretRef::numbered)
        .find(|candidate| !held.contains(candidate) && !values.contains_key(candidate))
        .unwrap_or_else(|| SecretRef::numbered(u128::MAX))
}

/// Clears every slot that holds `secret`: a field becomes unset, a raw option goes.
fn clear_secret(site: &mut SiteConfig, secret: &SecretRef) {
    for wlan in &mut site.wlans {
        if wlan.passphrase.as_ref() == Some(secret) {
            wlan.passphrase = None;
        }
    }
    if site.mesh.key.as_ref() == Some(secret) {
        site.mesh.key = None;
    }
    if let Some(password) = site.secret_mut("wan/password")
        && password.as_ref() == Some(secret)
    {
        *password = None;
    }
    for raw in site.nodes.iter_mut().flat_map(|entry| entry.raw.iter_mut()) {
        raw.options.retain(
            |_, value| !matches!(value, RawValue::Secret { secret: held } if held == secret),
        );
    }
}

#[cfg(test)]
#[path = "export_tests.rs"]
mod tests;
