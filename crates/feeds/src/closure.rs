//! The packages to keep of a feed set (the feeds one release, target and architecture's nodes
//! use, D-147): the wanted names with everything they depend on, as the package manager would
//! resolve them. A name is a package's own, else what a package provides; every package that
//! answers to it is kept (a virtual name has few providers, and the mirror can't know which one
//! a node would pick). Of a dependency's alternatives the first that some package answers to
//! is followed. Names nothing answers to are reported when they were wanted; a dependency that
//! none answers to is the image's (`kernel`, `libc` from another feed) and is left alone.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::index::{Entry, Index};

/// What [`resolve`] found.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Resolved {
    /// Each package to keep, as (index position, entry position), sorted.
    pub files: BTreeSet<(usize, usize)>,
    /// The wanted names no package answers to.
    pub missing: BTreeSet<String>,
}

/// The packages of `indexes` that `wanted` needs.
pub fn resolve<'a>(indexes: &[&Index], wanted: impl IntoIterator<Item = &'a str>) -> Resolved {
    let mut by_name: BTreeMap<&str, Vec<(usize, usize)>> = BTreeMap::new();
    let mut by_provides: BTreeMap<&str, Vec<(usize, usize)>> = BTreeMap::new();
    for (i, index) in indexes.iter().enumerate() {
        for (j, entry) in index.entries.iter().enumerate() {
            by_name.entry(&entry.name).or_default().push((i, j));
            for name in &entry.provides {
                by_provides.entry(name).or_default().push((i, j));
            }
        }
    }
    let answering = |name: &str| {
        by_name
            .get(name)
            .or_else(|| by_provides.get(name))
            .filter(|found| !found.is_empty())
    };
    let entry = |(i, j): (usize, usize)| -> &Entry { &indexes[i].entries[j] };

    let mut resolved = Resolved::default();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut queue: VecDeque<(String, bool)> = wanted
        .into_iter()
        .map(|name| (name.to_owned(), true))
        .collect();
    while let Some((name, top)) = queue.pop_front() {
        if !seen.insert(name.clone()) {
            continue;
        }
        let Some(found) = answering(&name) else {
            if top {
                resolved.missing.insert(name);
            }
            continue;
        };
        for &at in found {
            if !resolved.files.insert(at) {
                continue;
            }
            for alternatives in &entry(at).depends {
                if let Some(next) = alternatives.iter().find(|alt| answering(alt).is_some()) {
                    queue.push_back((next.clone(), false));
                }
            }
        }
    }
    resolved
}

/// Every package of `indexes` (the *Keep whole feeds* setting).
pub fn everything(indexes: &[&Index]) -> Resolved {
    Resolved {
        files: indexes
            .iter()
            .enumerate()
            .flat_map(|(i, index)| (0..index.entries.len()).map(move |j| (i, j)))
            .collect(),
        missing: BTreeSet::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, depends: &[&[&str]], provides: &[&str]) -> Entry {
        Entry {
            name: name.to_owned(),
            version: "1".to_owned(),
            file: format!("{name}.ipk"),
            size: 1,
            check: None,
            depends: depends
                .iter()
                .map(|alts| alts.iter().map(|&a| a.to_owned()).collect())
                .collect(),
            provides: provides.iter().map(|&p| p.to_owned()).collect(),
        }
    }

    fn names(indexes: &[&Index], resolved: &Resolved) -> Vec<String> {
        resolved
            .files
            .iter()
            .map(|&(i, j)| indexes[i].entries[j].name.clone())
            .collect()
    }

    #[test]
    fn dependencies_are_followed_across_feeds_through_provided_names() {
        let base = Index {
            entries: vec![
                entry("libc", &[], &[]),
                entry("libustream-mbedtls", &[&["libc"]], &["libustream-tls"]),
                entry("libustream-openssl", &[&["libc"]], &["libustream-tls"]),
                entry("unrelated", &[], &[]),
            ],
        };
        let packages = Index {
            entries: vec![
                entry(
                    "https-dns-proxy",
                    &[&["libc"], &["libustream-tls"], &["kernel"]],
                    &[],
                ),
                entry("tcpdump", &[&["libpcap1", "libc"]], &[]),
            ],
        };
        let indexes = [&base, &packages];
        let resolved = resolve(&indexes, ["https-dns-proxy", "tcpdump", "nope"]);
        assert_eq!(
            names(&indexes, &resolved),
            [
                "libc",
                "libustream-mbedtls",
                "libustream-openssl",
                "https-dns-proxy",
                "tcpdump"
            ]
        );
        // `kernel` is the image's: not missing, as it wasn't asked for.
        assert_eq!(resolved.missing, BTreeSet::from(["nope".to_owned()]));
        assert_eq!(everything(&indexes).files.len(), 6);
    }

    #[test]
    fn cycles_end() {
        let index = Index {
            entries: vec![entry("a", &[&["b"]], &[]), entry("b", &[&["a"]], &[])],
        };
        let resolved = resolve(&[&index], ["a"]);
        assert_eq!(resolved.files.len(), 2);
    }
}
