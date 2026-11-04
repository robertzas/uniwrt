//! The semantic diff of two versions of a package: which sections came, went, changed or moved,
//! and how each option changed.
//!
//! Semantics: a package is, per section type, a sequence of sections (`@type[0]`, `@type[1]`,
//! …), each a type, an optional name and a map of options; order matters within a type
//! (firewall rules apply in order), not between types, nor among a section's options. A list's
//! items keep their order. Two packages are the same when [`diff`] finds nothing.
//!
//! Matching: named sections pair by name (their type may change). Anonymous sections pair
//! within their type: equal ones first, as the longest common subsequence of each type's
//! sequence (after trimming the equal start and end), then, between those, the most similar
//! ones in order (half or more of their options alike, an option counting fully when equal and
//! half when only its name is), so an edited firewall rule reads as changed, not as one removed
//! and one added. A section that keeps its pair but falls out of its type's order (outside the
//! longest increasing run of old positions) is `moved`.

mod align;
mod apply;
mod show;
#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};

pub use self::apply::{ApplyError, apply};
use crate::ast::{Package, Section, Value};

/// What changed in a package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Diff {
    pub package: String,
    /// In the new package's order; a removed section comes where it was.
    pub sections: Vec<SectionDiff>,
}

/// A section that was added (no `before`), removed (no `after`), changed or moved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SectionDiff {
    pub before: Option<SectionId>,
    pub after: Option<SectionId>,
    /// It left its place among the sections of its type.
    #[serde(default)]
    pub moved: bool,
    /// Every option of an added or removed section; the changed ones of a changed section.
    pub options: Vec<OptionDiff>,
}

/// Where a section is: its type, its name and its place among the sections of its type, as
/// `uci` addresses an anonymous one (`@type[index]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SectionId {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub name: Option<String>,
    pub index: usize,
}

/// An option that was added (no `before`), removed (no `after`) or changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct OptionDiff {
    pub name: String,
    pub before: Option<Value>,
    pub after: Option<Value>,
}

impl Diff {
    pub fn is_empty(&self) -> bool {
        self.sections.is_empty()
    }
}

/// How `after` differs from `before` (named by `after`).
pub fn diff(before: &Package, after: &Package) -> Diff {
    let (a, b) = (&before.sections, &after.sections);
    let (ids_a, ids_b) = (ids(a), ids(b));
    let mut sections = Vec::new();
    for pair in align::pair(a, b) {
        let options = match (pair.a, pair.b) {
            (Some(i), Some(j)) => options(&a[i], &b[j]),
            (Some(i), None) => whole(&a[i], true),
            (None, Some(j)) => whole(&b[j], false),
            (None, None) => continue,
        };
        let kind_changed = matches!((pair.a, pair.b), (Some(i), Some(j)) if a[i].kind != b[j].kind);
        let both = pair.a.is_some() && pair.b.is_some();
        if both && options.is_empty() && !pair.moved && !kind_changed {
            continue;
        }
        sections.push(SectionDiff {
            before: pair.a.map(|i| ids_a[i].clone()),
            after: pair.b.map(|j| ids_b[j].clone()),
            moved: pair.moved,
            options,
        });
    }
    Diff {
        package: after.name.clone(),
        sections,
    }
}

/// Each section's id.
fn ids(sections: &[Section]) -> Vec<SectionId> {
    let mut seen = std::collections::HashMap::<&str, usize>::new();
    sections
        .iter()
        .map(|s| {
            let index = seen.entry(&s.kind).or_default();
            *index += 1;
            SectionId {
                kind: s.kind.clone(),
                name: s.name.clone(),
                index: *index - 1,
            }
        })
        .collect()
}

/// Every option of a section that came or went.
fn whole(section: &Section, removed: bool) -> Vec<OptionDiff> {
    section
        .options
        .iter()
        .map(|e| {
            let value = Some(e.value.clone());
            let (before, after) = if removed {
                (value, None)
            } else {
                (None, value)
            };
            OptionDiff {
                name: e.name.clone(),
                before,
                after,
            }
        })
        .collect()
}

/// The options that differ: the new section's in its order, then the removed ones.
fn options(a: &Section, b: &Section) -> Vec<OptionDiff> {
    let mut out: Vec<OptionDiff> = b
        .options
        .iter()
        .filter_map(|e| {
            let before = a.get(&e.name);
            (before != Some(&e.value)).then(|| OptionDiff {
                name: e.name.clone(),
                before: before.cloned(),
                after: Some(e.value.clone()),
            })
        })
        .collect();
    out.extend(
        a.options
            .iter()
            .filter(|e| b.get(&e.name).is_none())
            .map(|e| OptionDiff {
                name: e.name.clone(),
                before: Some(e.value.clone()),
                after: None,
            }),
    );
    out
}

/// Whether two sections are the same, their options in any order.
pub(crate) fn same(a: &Section, b: &Section) -> bool {
    a.kind == b.kind
        && a.name == b.name
        && a.options.len() == b.options.len()
        && a.options.iter().all(|e| b.get(&e.name) == Some(&e.value))
}
