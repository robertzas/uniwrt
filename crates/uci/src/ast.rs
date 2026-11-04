//! The AST: a package of sections, each a type, an optional name and its options in order.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::name::{is_name, is_package, is_type};

/// One `/etc/config` file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(rename = "UciPackage"))]
pub struct Package {
    pub name: String,
    pub sections: Vec<Section>,
}

/// A `config` section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Section {
    /// Its type, such as `interface`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Its name; `None` is an anonymous section, which `uci` addresses as `@type[index]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub name: Option<String>,
    /// Options and lists, in file order, each name once.
    #[serde(default)]
    pub options: Vec<Entry>,
}

/// An option or a list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Entry {
    pub name: String,
    pub value: Value,
}

/// A value: one string (`option`) or several (`list`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(untagged)]
pub enum Value {
    Single(String),
    List(Vec<String>),
}

impl Value {
    /// The string of an `option`.
    pub fn as_single(&self) -> Option<&str> {
        match self {
            Self::Single(value) => Some(value),
            Self::List(_) => None,
        }
    }

    /// The items: the list's, or the option's one value.
    pub fn items(&self) -> &[String] {
        match self {
            Self::Single(value) => std::slice::from_ref(value),
            Self::List(items) => items,
        }
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Self::Single(value.to_owned())
    }
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Self::Single(value)
    }
}

impl From<Vec<String>> for Value {
    fn from(items: Vec<String>) -> Self {
        Self::List(items)
    }
}

impl<const N: usize> From<[&str; N]> for Value {
    fn from(items: [&str; N]) -> Self {
        Self::List(items.iter().map(|&item| item.to_owned()).collect())
    }
}

impl Package {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            sections: Vec::new(),
        }
    }

    /// The named section.
    pub fn section(&self, name: &str) -> Option<&Section> {
        self.sections
            .iter()
            .find(|s| s.name.as_deref() == Some(name))
    }

    pub fn section_mut(&mut self, name: &str) -> Option<&mut Section> {
        self.sections
            .iter_mut()
            .find(|s| s.name.as_deref() == Some(name))
    }

    /// The sections of a type, in order: `@type[0]`, `@type[1]`, …
    pub fn sections_of<'a>(&'a self, kind: &'a str) -> impl Iterator<Item = &'a Section> + 'a {
        self.sections.iter().filter(move |s| s.kind == kind)
    }

    /// Whether `uci` reads [`Package::to_file`] back as this package: every name valid, named
    /// sections and each section's options unique, no empty `option` (libuci skips it) and no
    /// empty list, no NUL in a value.
    pub fn validate(&self) -> Result<(), Invalid> {
        let fail = |at: String, problem| Err(Invalid { at, problem });
        if !is_package(&self.name) {
            return fail(self.name.clone(), Problem::PackageName);
        }
        let mut named = std::collections::HashSet::new();
        for (index, section) in self.sections.iter().enumerate() {
            let at = || match &section.name {
                Some(name) => format!("{}.{name}", self.name),
                None => format!("{}.#{index}", self.name),
            };
            if !is_type(&section.kind) {
                return fail(at(), Problem::Type);
            }
            if let Some(name) = &section.name {
                if !is_name(name) {
                    return fail(at(), Problem::Name);
                }
                if !named.insert(name.as_str()) {
                    return fail(at(), Problem::DuplicateSection);
                }
            }
            for (i, entry) in section.options.iter().enumerate() {
                let at = || format!("{}.{}", at(), entry.name);
                if !is_name(&entry.name) {
                    return fail(at(), Problem::Name);
                }
                if section.options[..i].iter().any(|e| e.name == entry.name) {
                    return fail(at(), Problem::DuplicateOption);
                }
                match &entry.value {
                    Value::Single(value) if value.is_empty() => {
                        return fail(at(), Problem::EmptyOption);
                    }
                    Value::List(items) if items.is_empty() => {
                        return fail(at(), Problem::EmptyList);
                    }
                    value if value.items().iter().any(|item| item.contains('\0')) => {
                        return fail(at(), Problem::Nul);
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }
}

impl Section {
    /// An anonymous section.
    pub fn anonymous(kind: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            name: None,
            options: Vec::new(),
        }
    }

    pub fn named(kind: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            name: Some(name.into()),
            options: Vec::new(),
        }
    }

    /// The section with an option set, for building one in place.
    #[must_use]
    pub fn with(mut self, name: &str, value: impl Into<Value>) -> Self {
        self.set(name, value);
        self
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        self.options
            .iter()
            .find(|e| e.name == name)
            .map(|e| &e.value)
    }

    /// An `option`'s string.
    pub fn option(&self, name: &str) -> Option<&str> {
        self.get(name).and_then(Value::as_single)
    }

    /// Sets an option or a list, in its place if it exists, else last.
    pub fn set(&mut self, name: &str, value: impl Into<Value>) {
        let value = value.into();
        match self.options.iter_mut().find(|e| e.name == name) {
            Some(entry) => entry.value = value,
            None => self.options.push(Entry {
                name: name.to_owned(),
                value,
            }),
        }
    }

    /// Adds an item to a list as `uci add_list` does: an `option` becomes a list of its value
    /// and the item.
    pub fn add_list(&mut self, name: &str, item: impl Into<String>) {
        let item = item.into();
        match self.options.iter_mut().find(|e| e.name == name) {
            Some(entry) => match &mut entry.value {
                Value::List(items) => items.push(item),
                Value::Single(value) => {
                    entry.value = Value::List(vec![std::mem::take(value), item]);
                }
            },
            None => self.options.push(Entry {
                name: name.to_owned(),
                value: Value::List(vec![item]),
            }),
        }
    }

    pub fn remove(&mut self, name: &str) -> Option<Value> {
        let index = self.options.iter().position(|e| e.name == name)?;
        Some(self.options.remove(index).value)
    }
}

/// Why a [`Package`] would not read back as itself.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{at}: {problem}")]
pub struct Invalid {
    /// The package, section (`name` or `#index`) or option, such as `network.lan.proto`.
    pub at: String,
    pub problem: Problem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum Problem {
    #[error("a package name is [A-Za-z0-9_-]+")]
    PackageName,
    #[error("a section type is printable ASCII without spaces")]
    Type,
    #[error("a section or option name is [A-Za-z0-9_]+")]
    Name,
    #[error("another section has this name")]
    DuplicateSection,
    #[error("the section sets this option twice")]
    DuplicateOption,
    #[error("an empty option is no option")]
    EmptyOption,
    #[error("a list needs an item")]
    EmptyList,
    #[error("a value cannot hold a NUL")]
    Nul,
}
