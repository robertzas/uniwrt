//! Brownfield import's reading (features/lifecycle.md, Brownfield import; D-072, D-073): log in
//! to every adopted node, read the packages render writes and the facts render needs, run the
//! import engine on the site's latest revision, and seal the secrets it found. Nothing changes
//! on the nodes, nor in the site until the caller commits the draft.

use std::collections::{BTreeMap, BTreeSet};

use futures_util::future::join_all;
use uniwrt_core::NodeId;
use uniwrt_import::{Fate, NodeImport, Source};
use uniwrt_model::{
    apply::PlanIssue,
    facts::NodeFacts,
    import::{ImportPreview, ImportedSection, NodeImportState, NodeImportView, SectionFate},
    node::{Node, NodeMode, NodeRole},
    site::{
        SecretRef, SiteConfig, check_secret,
        diff::diff,
        node::NodeConfig,
        revision::Revision,
        validate::{Issue, Severity, validate},
    },
};
use uniwrt_node::{proto::ops::Set, txn::current};
use uniwrt_render::{RENDERABLE, Secrets, Target, render};
use uniwrt_store::{SealedSecret, VaultRepo as _};
use uniwrt_uci::Package;
use zeroize::Zeroizing;

use crate::{
    Error, Result,
    apply::{
        plan,
        survey::{self, Login},
    },
    errors::chain,
    runner::Shared,
};

/// An import ready to commit: the preview the user reviews (its `id` left for the caller to
/// give), the site it saves and the secrets sealed for it.
#[derive(Debug)]
pub struct ImportDraft {
    pub preview: ImportPreview,
    pub site: SiteConfig,
    /// The new secrets' sealed values, to commit with the revision; secrets the latest revision
    /// holds already keep their references.
    pub sealed: Vec<SealedSecret>,
}

/// A drift to import (features/lifecycle.md, Drift; D-074): only that package of that node is
/// taken as the node holds it, and every other package as the latest revision renders it, so
/// the preview shows only what the drift changes in the site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportScope {
    pub node: NodeId,
    pub package: String,
}

/// What a node holds.
struct Reading {
    packages: BTreeMap<String, Package>,
    facts: NodeFacts,
}

pub(crate) async fn draft(
    shared: &Shared,
    head: &Revision,
    mut nodes: Vec<Node>,
    scope: Option<&ImportScope>,
) -> Result<ImportDraft> {
    nodes.sort_by_key(|node| node.role != NodeRole::Gateway);
    let adopted: BTreeSet<NodeId> = nodes.iter().map(|node| node.id).collect();
    let order: Vec<Node> = nodes.clone();
    let (logins, failed) = survey::connect_all(shared, nodes).await;
    let readings = join_all(logins.iter().map(|login| read(shared, login))).await;
    let mut unread: BTreeMap<NodeId, String> = failed
        .into_iter()
        .map(|(node, why)| (node.id, why))
        .collect();
    let mut read = Vec::new();
    for (login, reading) in logins.into_iter().zip(readings) {
        login.remote.close().await;
        match reading {
            Ok(reading) => read.push((login.node, reading)),
            Err(why) => {
                unread.insert(login.node.id, why);
            }
        }
    }
    if let Some(scope) = scope {
        if let Some(why) = unread.get(&scope.node) {
            return Err(Error::Failed(format!("Cannot read the node: {why}")));
        }
        let secrets = survey::secrets(shared, &head.config)
            .await
            .map_err(|err| Error::Failed(err.message))?;
        read.retain_mut(
            |(node, reading)| match scoped(head, &secrets, node, reading, scope) {
                Ok(packages) => {
                    reading.packages = packages;
                    true
                }
                Err(why) => {
                    unread.insert(node.id, why);
                    false
                }
            },
        );
        if unread.contains_key(&scope.node) {
            return Err(Error::Failed(unread[&scope.node].clone()));
        }
    }
    let sources: Vec<Source<'_>> = read
        .iter()
        .map(|(node, reading)| Source {
            node: node.id,
            role: node.role,
            facts: &reading.facts,
            packages: &reading.packages,
        })
        .collect();
    let mut n = 0_u128;
    let mut mint = || {
        n += 1;
        SecretRef::numbered(n)
    };
    let import = uniwrt_import::import(&head.config, &sources, &mut mint);
    let mut site = import.site;
    let mut issues = Vec::new();
    let sealed = seal(shared, head, &mut site, import.secrets, &mut issues).await?;
    issues.extend(validate(&site, &adopted));
    let imported: BTreeMap<NodeId, NodeImport> = import
        .nodes
        .into_iter()
        .map(|node| (node.node, node))
        .collect();
    let nodes = order
        .iter()
        .map(|node| view(node, imported.get(&node.id), unread.remove(&node.id)))
        .collect();
    Ok(ImportDraft {
        preview: ImportPreview {
            id: String::new(),
            base: head.meta.id,
            changes: diff(&head.config, &site),
            issues,
            nodes,
        },
        site,
        sealed,
    })
}

/// The node's packages as the latest revision renders them (all of render's, as the import
/// engine fits them), the scope's package as the node holds it (or gone).
fn scoped(
    head: &Revision,
    secrets: &dyn Secrets,
    node: &Node,
    reading: &Reading,
    scope: &ImportScope,
) -> Result<BTreeMap<String, Package>, String> {
    let mut trial = head.config.clone();
    if let Some(config) = trial.nodes.iter_mut().find(|n| n.node == node.id) {
        config.managed = RENDERABLE.into_iter().collect();
    } else {
        let mut config = NodeConfig::new(node.id);
        config.managed = RENDERABLE.into_iter().collect();
        trial.nodes.push(config);
    }
    let target = Target {
        node: node.id,
        role: node.role,
        facts: &reading.facts,
        current: &reading.packages,
    };
    let rendered = render(&trial, secrets, target).map_err(|issues| {
        let why: Vec<String> = issues.into_iter().map(|issue| issue.message).collect();
        format!(
            "The latest revision doesn't render for it: {}",
            why.join("; ")
        )
    })?;
    let mut packages: BTreeMap<String, Package> = rendered
        .packages
        .into_iter()
        .map(|(package, rendered)| (package.name().to_owned(), rendered))
        .collect();
    if node.id == scope.node {
        match reading.packages.get(&scope.package) {
            Some(kept) => {
                packages.insert(scope.package.clone(), kept.clone());
            }
            None => {
                packages.remove(&scope.package);
            }
        }
    }
    Ok(packages)
}

async fn read(shared: &Shared, login: &Login) -> Result<Reading, String> {
    let remote = login.remote.as_ref();
    if login.node.mode == NodeMode::Managed {
        survey::idle_tool(remote).await?;
    }
    let read = current::packages(&plan::packages()).map_err(|err| err.to_string())?;
    let files = remote
        .exec(read.request.clone())
        .await
        .and_then(|output| read.parse(output))
        .map_err(|err| format!("Cannot read its configuration: {}", chain(&err)))?;
    let facts = survey::collect(shared, remote, &Set::ALL, &survey::RENDER_SETS).await?;
    let mut packages = BTreeMap::new();
    for (name, text) in files {
        let Some(text) = text else { continue };
        let package = uniwrt_uci::parse(&name, &text)
            .map_err(|err| format!("Cannot parse its /etc/config/{name}: {err}"))?;
        packages.insert(name, package);
    }
    Ok(Reading { packages, facts })
}

/// Seals each secret the import found under its own name and points the site at it, or keeps
/// the latest revision's reference where that holds the same value already; a value that doesn't
/// suit its slot is an error in `issues`.
async fn seal(
    shared: &Shared,
    head: &Revision,
    site: &mut SiteConfig,
    found: BTreeMap<SecretRef, String>,
    issues: &mut Vec<Issue>,
) -> Result<Vec<SealedSecret>> {
    let paths: BTreeMap<SecretRef, String> = site
        .secrets()
        .into_iter()
        .filter_map(|slot| Some((slot.value?.clone(), slot.path)))
        .collect();
    let mut sealed = Vec::new();
    for (placeholder, value) in found {
        let value = Zeroizing::new(value);
        let Some(path) = paths.get(&placeholder) else {
            continue;
        };
        if let Err(why) = check_secret(path, &value) {
            issues.push(Issue {
                path: path.clone(),
                severity: Severity::Error,
                message: format!("{why} (as the node holds it)"),
            });
            continue;
        }
        let kept = match head.config.secret(path) {
            Some(old) => shared
                .vault
                .secret(old.name())
                .await?
                .is_some_and(|held| held.as_slice() == value.as_bytes())
                .then(|| old.clone()),
            None => None,
        };
        let reference = if let Some(old) = kept {
            old
        } else {
            let secret = shared.vault.seal_config_secret(value.as_bytes())?;
            let reference = secret.reference().clone();
            sealed.push(secret);
            reference
        };
        site.replace_secret(&placeholder, &reference);
    }
    Ok(sealed)
}

fn view(node: &Node, import: Option<&NodeImport>, reason: Option<String>) -> NodeImportView {
    let issue = |issue: &uniwrt_render::Issue| PlanIssue {
        path: issue.path.clone(),
        message: issue.message.clone(),
    };
    let mut view = NodeImportView {
        node: node.id,
        name: node.name.clone(),
        role: node.role,
        mode: node.mode,
        state: NodeImportState::Unread,
        reason,
        confidence: 0,
        sections: Vec::new(),
        residual: Vec::new(),
        issues: Vec::new(),
        warnings: Vec::new(),
    };
    let Some(import) = import else {
        return view;
    };
    view.state = if import.issues.is_empty() {
        NodeImportState::Imported
    } else {
        NodeImportState::Failed
    };
    view.confidence = import.confidence;
    view.sections = import
        .sections
        .iter()
        .map(|section| {
            let (fate, kept) = match &section.fate {
                Fate::Mapped { kept } => (SectionFate::Mapped, kept.clone()),
                Fate::Raw => (SectionFate::Raw, Vec::new()),
                Fate::Dropped => (SectionFate::Dropped, Vec::new()),
            };
            ImportedSection {
                package: section.package.clone(),
                kind: section.kind.clone(),
                name: section.name.clone(),
                renamed: section.renamed.clone(),
                fate,
                kept,
            }
        })
        .collect();
    view.residual.clone_from(&import.residual);
    view.residual.iter_mut().for_each(plan::mask);
    view.issues = import.issues.iter().map(issue).collect();
    view.warnings = import.warnings.iter().map(issue).collect();
    view
}

#[cfg(test)]
#[path = "import_tests.rs"]
mod tests;
