//! The packages an upgrade image keeps (pure; M7.1a1, D-121): what the node runs on purpose,
//! renamed as ASU's published changes say, beyond the new profile's defaults; what the new
//! release lacks is dropped (and listed, so it is seen before anything is flashed); the mesh
//! essentials are added as for a zero-touch image. The profile's defaults are never removed,
//! even ones the node seems to lack: a package list read short must not leave a driver or the
//! package manager out of the image. Only a package that replaces another (the full wpad for
//! the basic or mesh one) takes that one out.

use std::collections::BTreeSet;

use crate::{
    catalog::{PackageChange, ProfileInfo},
    packages::packages,
};

/// What a plan works from.
pub(crate) struct Inputs<'a> {
    /// Installed on purpose (the user's or the image's list).
    pub(crate) explicit: &'a BTreeSet<String>,
    /// The defaults of the node's profile in the release it runs, when known: those the new
    /// profile dropped (opkg on 25.12) aren't carried over.
    pub(crate) old_defaults: Option<&'a BTreeSet<String>>,
    /// The defaults of its profile in the new release.
    pub(crate) new_defaults: &'a BTreeSet<String>,
    /// The new release's packages for the device (its architecture's and its target's).
    pub(crate) available: &'a BTreeSet<String>,
    /// The new branch's renames, and the main line's revisions they span: where the node's
    /// branch and the new one left it (equal within a branch).
    pub(crate) changes: &'a [PackageChange],
    pub(crate) from_revision: Option<u32>,
    pub(crate) to_revision: Option<u32>,
}

/// What the image holds against the node, sorted.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ImagePackages {
    /// ASU's list: added to the defaults, or removed with a leading `-`.
    pub(crate) packages: Vec<String>,
    pub(crate) kept: Vec<String>,
    pub(crate) renamed: Vec<(String, String)>,
    pub(crate) added: Vec<String>,
    pub(crate) dropped: Vec<String>,
    /// Replaced by a package the image holds (a basic or mesh wpad by the full one).
    pub(crate) removed: Vec<String>,
}

/// Packages that aren't installed from a feed.
const NOT_PACKAGES: [&str; 1] = ["kernel"];

pub(crate) fn plan(inputs: &Inputs<'_>) -> ImagePackages {
    let mut kept = BTreeSet::new();
    let mut renamed = Vec::new();
    let mut dropped = BTreeSet::new();
    for name in inputs.explicit {
        if NOT_PACKAGES.contains(&name.as_str()) {
            continue;
        }
        let Some(to) = follow(name, inputs) else {
            dropped.insert(name.clone());
            continue;
        };
        if to != *name {
            renamed.push((name.clone(), to.clone()));
        }
        // In the image anyway, or only there as a default the new profile no longer has.
        if inputs.new_defaults.contains(&to)
            || inputs
                .old_defaults
                .is_some_and(|defaults| defaults.contains(name))
        {
            continue;
        }
        if inputs.available.contains(&to) {
            kept.insert(to);
        } else {
            dropped.insert(name.clone());
        }
    }
    let mut image: BTreeSet<String> = inputs.new_defaults.union(&kept).cloned().collect();
    let mut removed = BTreeSet::new();
    // A full wpad kept from the node replaces the basic and mesh ones.
    if image.iter().any(|name| full_wpad(name)) {
        removed.extend(image.iter().filter(|name| partial_wpad(name)).cloned());
    }
    image.retain(|name| !removed.contains(name));
    let mut added = BTreeSet::new();
    for package in packages(&ProfileInfo { packages: image }) {
        match package.strip_prefix('-') {
            Some(name) => {
                removed.insert(name.to_owned());
            }
            None => {
                added.insert(package);
            }
        }
    }
    kept.retain(|name| !removed.contains(name));
    let packages = kept
        .iter()
        .chain(&added)
        .cloned()
        .chain(
            removed
                .iter()
                .filter(|name| inputs.new_defaults.contains(*name))
                .map(|name| format!("-{name}")),
        )
        .collect();
    ImagePackages {
        packages,
        kept: kept.into_iter().collect(),
        renamed,
        added: added.into_iter().collect(),
        dropped: dropped.into_iter().collect(),
        removed: removed.into_iter().collect(),
    }
}

/// The name `name` has in the new release, following renames in order; `None` when it was
/// retired without a successor.
fn follow(name: &str, inputs: &Inputs<'_>) -> Option<String> {
    let mut name = name.to_owned();
    let mut changes: Vec<&PackageChange> = inputs
        .changes
        .iter()
        .filter(|change| !change.mandatory)
        .filter(|change| {
            inputs
                .from_revision
                .is_none_or(|from| change.revision > from)
        })
        .filter(|change| inputs.to_revision.is_none_or(|to| change.revision <= to))
        .collect();
    changes.sort_by_key(|change| change.revision);
    for change in changes {
        if change.source == name {
            name = change.target.clone()?;
        }
    }
    Some(name)
}

/// The basic and mesh wpads, which the full one replaces.
fn partial_wpad(name: &str) -> bool {
    name.starts_with("wpad-basic-") || name.starts_with("wpad-mesh-")
}

fn full_wpad(name: &str) -> bool {
    name == "wpad"
        || ["mbedtls", "wolfssl", "openssl"]
            .iter()
            .any(|tls| name.strip_prefix("wpad-") == Some(tls))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|n| (*n).to_owned()).collect()
    }

    fn change(source: &str, target: Option<&str>, revision: u32) -> PackageChange {
        PackageChange {
            source: source.to_owned(),
            target: target.map(str::to_owned),
            revision,
            mandatory: false,
        }
    }

    struct Case {
        explicit: BTreeSet<String>,
        old: Option<BTreeSet<String>>,
        new: BTreeSet<String>,
        available: BTreeSet<String>,
        changes: Vec<PackageChange>,
    }

    impl Case {
        fn plan(&self) -> ImagePackages {
            plan(&Inputs {
                explicit: &self.explicit,
                old_defaults: self.old.as_ref(),
                new_defaults: &self.new,
                available: &self.available,
                changes: &self.changes,
                from_revision: Some(27990),
                to_revision: Some(32295),
            })
        }
    }

    /// A 24.10 node with the full wpad, the mesh, LuCI and a package from another feed,
    /// upgraded to 25.12 (opkg becomes apk).
    fn case() -> Case {
        let old = set(&["dnsmasq", "opkg", "ppp", "wpad-basic-mbedtls"]);
        Case {
            explicit: set(&[
                "dnsmasq",
                "opkg",
                "kernel",
                "luci",
                "wpad-mbedtls",
                "kmod-batman-adv",
                "batctl-default",
                "custom-feed-tool",
            ]),
            old: Some(old),
            new: set(&["apk-mbedtls", "dnsmasq", "ppp", "wpad-basic-mbedtls"]),
            available: set(&["luci", "wpad-mbedtls", "kmod-batman-adv", "batctl-default"]),
            changes: vec![
                change("opkg", Some("apk-mbedtls"), 28056),
                change("auc", Some("owut"), 26792),
                change("firewall", Some("firewall4"), 18611),
            ],
        }
    }

    #[test]
    fn the_nodes_packages_are_kept_renamed_and_the_missing_dropped() {
        let kept = case().plan();
        assert_eq!(
            kept.kept,
            ["batctl-default", "kmod-batman-adv", "luci", "wpad-mbedtls"]
        );
        assert_eq!(
            kept.renamed,
            [("opkg".to_owned(), "apk-mbedtls".to_owned())]
        );
        assert_eq!(kept.dropped, ["custom-feed-tool"]);
        assert_eq!(kept.removed, ["wpad-basic-mbedtls"]);
        assert_eq!(kept.added, [] as [std::string::String; 0]);
        assert_eq!(
            kept.packages,
            [
                "batctl-default",
                "kmod-batman-adv",
                "luci",
                "wpad-mbedtls",
                "-wpad-basic-mbedtls"
            ]
        );
    }

    #[test]
    fn defaults_are_never_removed_even_ones_the_node_seems_to_lack() {
        let mut case = case();
        case.explicit.remove("dnsmasq");
        case.new.insert("kmod-mt7915e".to_owned());
        let kept = case.plan();
        assert_eq!(kept.removed, ["wpad-basic-mbedtls"]);
        assert!(
            !kept
                .packages
                .iter()
                .any(|p| p == "-ppp" || p == "-kmod-mt7915e")
        );
    }

    #[test]
    fn without_the_old_profile_the_wpad_still_swaps() {
        let mut case = case();
        case.old = None;
        let kept = case.plan();
        assert_eq!(kept.removed, ["wpad-basic-mbedtls"]);
        assert!(kept.kept.contains(&"luci".to_owned()));
        // opkg is carried over by its new name, a default of the new profile.
        assert!(
            !kept
                .kept
                .iter()
                .any(|p| p.starts_with("apk") || p == "opkg")
        );
    }

    #[test]
    fn a_mesh_wpad_is_replaced_by_the_full_one_not_kept() {
        let mut case = case();
        case.explicit.remove("wpad-mbedtls");
        case.explicit.insert("wpad-mesh-mbedtls".to_owned());
        case.available.insert("wpad-mesh-mbedtls".to_owned());
        let kept = case.plan();
        assert!(!kept.kept.contains(&"wpad-mesh-mbedtls".to_owned()));
        assert_eq!(kept.added, ["wpad-mbedtls"]);
        assert_eq!(kept.removed, ["wpad-basic-mbedtls", "wpad-mesh-mbedtls"]);
        assert!(!kept.packages.contains(&"wpad-mesh-mbedtls".to_owned()));
        assert!(
            !kept.packages.contains(&"-wpad-mesh-mbedtls".to_owned()),
            "not a default"
        );
        assert!(kept.packages.contains(&"-wpad-basic-mbedtls".to_owned()));
    }

    #[test]
    fn a_node_without_the_mesh_gets_its_essentials() {
        let mut case = case();
        for name in ["wpad-mbedtls", "kmod-batman-adv", "batctl-default"] {
            case.explicit.remove(name);
        }
        let kept = case.plan();
        assert_eq!(
            kept.added,
            ["batctl-default", "kmod-batman-adv", "wpad-mbedtls"]
        );
        assert_eq!(kept.removed, ["wpad-basic-mbedtls"]);
        assert_eq!(kept.kept, ["luci"]);
    }

    #[test]
    fn renames_outside_the_span_dont_apply_and_chains_follow() {
        let mut case = case();
        case.explicit.insert("firewall".to_owned());
        case.available.insert("firewall".to_owned());
        case.explicit.insert("a".to_owned());
        case.available.insert("c".to_owned());
        case.changes.push(change("a", Some("b"), 30000));
        case.changes.push(change("b", Some("c"), 31000));
        case.changes.push(change("gone", None, 30000));
        case.explicit.insert("gone".to_owned());
        let kept = case.plan();
        assert!(kept.kept.contains(&"firewall".to_owned()));
        assert!(kept.kept.contains(&"c".to_owned()));
        assert!(kept.renamed.contains(&("a".to_owned(), "c".to_owned())));
        assert!(kept.dropped.contains(&"gone".to_owned()));
    }
}
