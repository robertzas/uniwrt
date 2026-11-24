//! The UCI explorer's view of a node's files (features/observability.md, Diagnostics toolkit ›
//! UCI explorer; D-143): [`tree`] parses each package the node holds, masks the values of
//! secret options ([`is_masked_option`]), says who writes each package ([`UciOwner`]) and
//! places each of the node's raw sections on the section it finds, as render would find it
//! ([`raw::target`]).

use std::collections::BTreeMap;

use uniwrt_model::{
    apply::MASKED,
    explorer::{UciOwner, UciTreeOption, UciTreePackage, UciTreeSection, is_masked_option},
    site::node::{NodeConfig, RawSection, RawValue},
};
use uniwrt_uci::{Package, Section, Value};

use crate::raw;

/// The node's packages and the raw sections that find no section in them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tree {
    /// By name.
    pub packages: Vec<UciTreePackage>,
    /// Indices in `NodeConfig::raw`, in its order.
    pub unmatched: Vec<u32>,
}

/// The tree of `files`, the node's `/etc/config` by package (its text, or why it isn't text),
/// with `config`, the node's configuration in the site's latest revision, if it has one.
/// `managed`: the node is managed; in observe mode every package is its own.
pub fn tree(
    files: &BTreeMap<String, Result<String, String>>,
    config: Option<&NodeConfig>,
    managed: bool,
) -> Tree {
    let mut packages = Vec::with_capacity(files.len());
    let mut parsed = Vec::with_capacity(files.len());
    for (name, text) in files {
        let read = text
            .clone()
            .and_then(|text| uniwrt_uci::parse(name, &text).map_err(|err| err.to_string()));
        let (sections, error) = match &read {
            Ok(package) => (package.sections.iter().map(section).collect(), None),
            Err(error) => (Vec::new(), Some(error.clone())),
        };
        packages.push(UciTreePackage {
            name: name.clone(),
            owner: owner(name, config, managed),
            sections,
            error,
        });
        parsed.push(read.ok());
    }
    let mut unmatched = Vec::new();
    for (i, raw) in config.map_or(&[][..], |c| &c.raw).iter().enumerate() {
        let index = u32::try_from(i).unwrap_or(u32::MAX);
        let found = packages
            .iter()
            .position(|package| package.name == raw.package)
            .and_then(|p| Some((p, finds(parsed[p].as_ref()?, raw)?)));
        match found {
            Some((p, s)) => packages[p].sections[s].overrides.push(index),
            None => unmatched.push(index),
        }
    }
    Tree {
        packages,
        unmatched,
    }
}

fn owner(name: &str, config: Option<&NodeConfig>, managed: bool) -> UciOwner {
    let Some(config) = config.filter(|_| managed) else {
        return UciOwner::Node;
    };
    if config.managed.iter().any(|package| package.name() == name) {
        UciOwner::Rendered
    } else if config.raw.iter().any(|raw| raw.package == name) {
        UciOwner::Patched
    } else {
        UciOwner::Node
    }
}

/// The section `raw` stands for in `package`: the one a merge or removal finds, the one of a
/// whole section's name, or for an anonymous whole section the first of its type that holds
/// its plain options (one without options: the first of its type without any).
fn finds(package: &Package, raw: &RawSection) -> Option<usize> {
    if raw.finds() {
        return raw::target(package, raw);
    }
    if let Some(name) = &raw.name {
        return package
            .sections
            .iter()
            .position(|s| s.name.as_ref() == Some(name));
    }
    let plain: Vec<_> = raw
        .options
        .iter()
        .filter(|(_, value)| !matches!(value, RawValue::Secret { .. }))
        .collect();
    package.sections.iter().position(|s| {
        s.kind == raw.kind
            && s.name.is_none()
            && if plain.is_empty() {
                s.options.is_empty()
            } else {
                plain.iter().all(|(name, value)| raw::holds(s, name, value))
            }
    })
}

fn section(section: &Section) -> UciTreeSection {
    UciTreeSection {
        kind: section.kind.clone(),
        name: section.name.clone(),
        options: section
            .options
            .iter()
            .map(|entry| {
                let secret = is_masked_option(&entry.name);
                let value = match &entry.value {
                    Value::Single(_) if secret => Value::Single(MASKED.to_owned()),
                    Value::List(items) if secret => {
                        Value::List(vec![MASKED.to_owned(); items.len()])
                    }
                    value => value.clone(),
                };
                UciTreeOption {
                    name: entry.name.clone(),
                    value,
                    secret,
                }
            })
            .collect(),
        overrides: Vec::new(),
    }
}

#[cfg(test)]
mod tests;
