//! An import's secrets (D-130): each slot the file sets takes the file's value, the latest
//! revision's at the same path, or none (a VPN key the accept makes, D-140); the file's
//! references never meet the vault's.

use std::collections::{BTreeMap, BTreeSet};

use zeroize::Zeroizing;

use super::import::{Head, SecretImport, SecretSource};
use crate::site::{
    SecretRef, SiteConfig, check_secret,
    node::RawValue,
    validate::{Issue, Severity},
    vpn,
};

/// Points every secret slot the imported document sets at its value: the file's (under a
/// placeholder, returned to seal, unless the latest revision holds it at the same path), the
/// latest revision's at the same path, or none.
pub(super) fn resolve_secrets(
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
                    if vpn::generated(&path) {
                        SecretSource::Generated
                    } else {
                        SecretSource::Missing
                    }
                }
            }
        };
        sources.push(SecretImport { path, source });
    }
    (sources, sealed)
}

/// A placeholder reference that neither `site` nor `values` holds yet.
pub(super) fn placeholder(
    site: &SiteConfig,
    values: &BTreeMap<SecretRef, Zeroizing<String>>,
) -> SecretRef {
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
    let paths: Vec<String> = site
        .secrets()
        .into_iter()
        .filter(|slot| slot.value == Some(secret))
        .map(|slot| slot.path)
        .collect();
    for path in paths {
        if let Some(slot) = site.secret_mut(&path) {
            *slot = None;
        }
    }
    for raw in site.nodes.iter_mut().flat_map(|entry| entry.raw.iter_mut()) {
        raw.options.retain(
            |_, value| !matches!(value, RawValue::Secret { secret: held } if held == secret),
        );
    }
}
