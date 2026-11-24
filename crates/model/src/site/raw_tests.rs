//! Raw sections' removals, unset options and matched anonymous sections (D-143): their
//! validation and the v19 migration.

use std::collections::BTreeSet;

use serde_json::Value;
use uniwrt_core::NodeId;

use super::{
    SiteConfig,
    migrate::upgrade,
    node::{RawSection, RawValue},
    validate::{Severity, validate},
};

fn fixture(version: u32) -> SiteConfig {
    let text = match version {
        18 => include_str!("../../fixtures/site/v18.json"),
        _ => include_str!("../../fixtures/site/v19.json"),
    };
    upgrade(serde_json::from_str::<Value>(text).unwrap()).unwrap()
}

/// The raw sections' errors after the first node's are `raw`, by path.
fn errors_with(raw: Vec<RawSection>) -> Vec<String> {
    let mut config = fixture(19);
    config.nodes[0].raw = raw;
    let nodes: BTreeSet<NodeId> = [NodeId::new(1), NodeId::new(2)].into();
    validate(&config, &nodes)
        .into_iter()
        .filter(|issue| issue.path.contains("/raw/"))
        .inspect(|issue| assert_eq!(issue.severity, Severity::Error, "{issue:?}"))
        .map(|issue| issue.path)
        .collect()
}

fn one(value: &str) -> RawValue {
    RawValue::Option(value.to_owned())
}

#[test]
fn v19_adds_nothing_to_remove_unset_or_match() {
    let config = fixture(18);
    assert!(
        config
            .nodes
            .iter()
            .flat_map(|n| &n.raw)
            .all(|raw| { raw.unset.is_empty() && !raw.remove && raw.matching.is_empty() })
    );
}

#[test]
fn any_package_may_be_patched_without_a_warning() {
    let mut luci = RawSection::new("luci", "core", Some("main".to_owned()));
    luci.options.insert("lang".to_owned(), one("en"));
    assert_eq!(errors_with(vec![luci]), Vec::<String>::new());
}

#[test]
fn removals_unsets_and_matches_are_checked() {
    let mut both = RawSection::new("network", "route", None);
    (both.merge, both.remove) = (true, true);
    let mut named = RawSection::new("network", "interface", Some("guest".to_owned()));
    named.remove = true;
    named.options.insert("proto".to_owned(), one("static"));
    let mut unmerged = RawSection::new("network", "interface", Some("lan".to_owned()));
    unmerged.unset.insert("mtu".to_owned());
    let mut twice = RawSection::new("network", "interface", Some("lan".to_owned()));
    twice.merge = true;
    twice.options.insert("mtu".to_owned(), one("1400"));
    twice
        .unset
        .extend(["mtu".to_owned(), "bad-name".to_owned()]);
    let mut added = RawSection::new("network", "route", None);
    added
        .matching
        .insert("target".to_owned(), one("10.0.0.0/8"));
    let mut secret = RawSection::new("wireless", "wifi-iface", None);
    secret.merge = true;
    secret.matching.insert("key".to_owned(), one("hunter22"));
    secret.matching.insert("ssid".to_owned(), one("a\nb"));
    let mut fine = RawSection::new("firewall", "rule", None);
    fine.remove = true;
    fine.matching.insert("src".to_owned(), one("wan"));
    assert_eq!(
        errors_with(vec![both, named, unmerged, twice, added, secret, fine]),
        [
            "nodes/1/raw/0",
            "nodes/1/raw/1/options",
            "nodes/1/raw/2/unset/mtu",
            "nodes/1/raw/3/unset/bad-name",
            "nodes/1/raw/3/unset/mtu",
            "nodes/1/raw/4/matching",
            "nodes/1/raw/5/matching/key",
            "nodes/1/raw/5/matching/ssid",
        ]
    );
}
