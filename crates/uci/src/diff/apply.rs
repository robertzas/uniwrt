//! Applying a diff to the package it was taken from: `apply(a, &diff(a, b))` is `b` up to the
//! diff's semantics (the order between types and among a section's options may differ).

use std::collections::{HashMap, VecDeque};

use thiserror::Error;

use super::{Diff, SectionId};
use crate::ast::{Entry, Package, Section};

/// Why a diff does not apply: the package is not the one it was taken from.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ApplyError {
    #[error("{0}: no such section")]
    Missing(String),
    #[error("{0}: the section is not as the diff found it")]
    Conflict(String),
    #[error("{0}: two sections for one place")]
    Clash(String),
}

/// `before` with `diff` applied. New sections of a type go after the last old one of it, and
/// sections of a new type go last.
pub fn apply(before: &Package, diff: &Diff) -> Result<Package, ApplyError> {
    let a = &before.sections;
    let (work, placed) = changes(a, diff)?;

    // Each type's new sequence: placed sections at their index, the rest in their old order.
    let mut kept: HashMap<&str, VecDeque<Section>> = HashMap::new();
    for (i, section) in work.into_iter().enumerate() {
        if let Some(section) = section {
            kept.entry(&a[i].kind).or_default().push_back(section);
        }
    }
    let mut kinds: Vec<&str> = a.iter().map(|s| s.kind.as_str()).collect();
    kinds.extend(placed.iter().map(|(id, _)| id.kind.as_str()));
    let mut order: Vec<&str> = Vec::new();
    for kind in kinds {
        if !order.contains(&kind) {
            order.push(kind);
        }
    }
    let mut slots: HashMap<&str, Vec<Option<Section>>> = HashMap::new();
    for &kind in &order {
        let count = kept.get(kind).map_or(0, VecDeque::len)
            + placed.iter().filter(|(id, _)| id.kind == kind).count();
        slots.insert(kind, (0..count).map(|_| None).collect());
    }
    for (id, section) in placed {
        let slot = slots
            .get_mut(id.kind.as_str())
            .and_then(|s| s.get_mut(id.index))
            .filter(|slot| slot.is_none())
            .ok_or_else(|| ApplyError::Clash(id.to_string()))?;
        *slot = Some(section);
    }
    let mut sequences: HashMap<&str, VecDeque<Section>> = HashMap::new();
    for (kind, slots) in slots {
        let mut rest = kept.remove(kind).unwrap_or_default();
        let sequence = slots
            .into_iter()
            .filter_map(|slot| slot.or_else(|| rest.pop_front()));
        sequences.insert(kind, sequence.collect());
    }

    // Interleaved as before: each old section's place takes the next of its type.
    let mut keyed: Vec<((usize, usize), Section)> = Vec::new();
    let mut last: HashMap<&str, usize> = HashMap::new();
    for (i, old) in a.iter().enumerate() {
        if let Some(section) = sequences
            .get_mut(old.kind.as_str())
            .and_then(VecDeque::pop_front)
        {
            keyed.push(((i, 0), section));
            last.insert(&old.kind, i);
        }
    }
    let mut end = 0;
    for kind in order {
        for (k, section) in sequences
            .remove(kind)
            .unwrap_or_default()
            .into_iter()
            .enumerate()
        {
            let key = if let Some(&at) = last.get(kind) {
                (at, k + 1)
            } else {
                end += 1;
                (a.len(), end)
            };
            keyed.push((key, section));
        }
    }
    keyed.sort_by_key(|(key, _)| *key);
    Ok(Package {
        name: before.name.clone(),
        sections: keyed.into_iter().map(|(_, section)| section).collect(),
    })
}

/// The old sections no change names (the rest are `None`), and the changed and added ones with
/// where they go.
type Changes<'d> = (Vec<Option<Section>>, Vec<(&'d SectionId, Section)>);

fn changes<'d>(a: &[Section], diff: &'d Diff) -> Result<Changes<'d>, ApplyError> {
    let mut work: Vec<Option<Section>> = a.iter().cloned().map(Some).collect();
    let mut mentioned = vec![false; a.len()];
    let mut placed: Vec<(&SectionId, Section)> = Vec::new();
    for change in &diff.sections {
        let section = match &change.before {
            Some(id) => {
                let i = locate(a, id).ok_or_else(|| ApplyError::Missing(id.to_string()))?;
                if std::mem::replace(&mut mentioned[i], true) {
                    return Err(ApplyError::Clash(id.to_string()));
                }
                if change
                    .options
                    .iter()
                    .any(|o| a[i].get(&o.name) != o.before.as_ref())
                {
                    return Err(ApplyError::Conflict(id.to_string()));
                }
                let Some(mut section) = work[i].take() else {
                    return Err(ApplyError::Clash(id.to_string()));
                };
                for option in &change.options {
                    match &option.after {
                        Some(value) => section.set(&option.name, value.clone()),
                        None => {
                            section.remove(&option.name);
                        }
                    }
                }
                section
            }
            None => Section {
                kind: String::new(),
                name: None,
                options: change
                    .options
                    .iter()
                    .filter_map(|o| {
                        Some(Entry {
                            name: o.name.clone(),
                            value: o.after.clone()?,
                        })
                    })
                    .collect(),
            },
        };
        if let Some(id) = &change.after {
            let mut section = section;
            section.kind.clone_from(&id.kind);
            section.name.clone_from(&id.name);
            placed.push((id, section));
        }
    }
    Ok((work, placed))
}

/// The section an id names.
fn locate(sections: &[Section], id: &SectionId) -> Option<usize> {
    let i = (0..sections.len())
        .filter(|&i| sections[i].kind == id.kind)
        .nth(id.index)?;
    (sections[i].name == id.name).then_some(i)
}
