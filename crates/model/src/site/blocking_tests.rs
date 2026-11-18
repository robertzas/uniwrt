//! DNS filtering (schema v13, D-133): migration and validation.

use std::collections::BTreeSet;

use serde_json::Value;
use uniwrt_core::NodeId;

use super::{
    SiteConfig,
    blocking::{Blocking, CustomList, DEFAULT_LISTS},
    migrate::upgrade,
    validate::{Severity, validate},
};

fn fixture(version: u32) -> Value {
    let text = match version {
        12 => include_str!("../../fixtures/site/v12.json"),
        _ => include_str!("../../fixtures/site/v13.json"),
    };
    serde_json::from_str(text).unwrap()
}

/// The (path, severity) of the issues under `services/blocking` of the v13 fixture after
/// `edit`.
fn issues_after(edit: impl FnOnce(&mut Blocking)) -> Vec<(String, Severity)> {
    let mut config: SiteConfig = upgrade(fixture(13)).unwrap();
    edit(&mut config.services.blocking);
    validate(&config, &BTreeSet::from([NodeId::new(1), NodeId::new(2)]))
        .into_iter()
        .filter(|issue| issue.path.starts_with("services/blocking"))
        .map(|issue| (issue.path, issue.severity))
        .collect()
}

fn error(path: &str) -> (String, Severity) {
    (path.to_owned(), Severity::Error)
}

#[test]
fn v13_starts_off_with_the_default_lists_and_no_bypass() {
    let config = upgrade(fixture(12)).unwrap();
    let blocking = &config.services.blocking;
    assert!(!blocking.enabled && !blocking.bypasses());
    assert_eq!(blocking.lists, DEFAULT_LISTS);
    assert!(blocking.custom.is_empty() && blocking.allow.is_empty());
    assert!(blocking.deny.is_empty() && blocking.bypass.is_empty());
}

#[test]
fn the_fixture_bypasses_while_on() {
    let mut config = upgrade(fixture(13)).unwrap();
    assert!(config.services.blocking.bypasses());
    config.services.blocking.enabled = false;
    assert!(!config.services.blocking.bypasses());
}

#[test]
fn lists_come_from_the_catalog_once_each() {
    let issues = issues_after(|b| {
        b.lists = vec!["hagezi_pro".into(), "nope".into(), "hagezi_pro".into()];
    });
    assert_eq!(
        issues,
        [
            error("services/blocking/lists/1"),
            error("services/blocking/lists/2")
        ]
    );
}

#[test]
fn custom_lists_are_named_http_urls_once_each() {
    let list = |name: &str, url: &str| CustomList {
        name: name.to_owned(),
        url: url.to_owned(),
    };
    let issues = issues_after(|b| {
        b.custom = vec![
            list("Mine", "http://lists.example.com/a.txt"),
            list(" ", "https://lists.example.com/b.txt"),
            list("Bad", "ftp://lists.example.com/c.txt"),
            list("Quote", "https://lists.example.com/'d"),
            list("Again", "http://lists.example.com/a.txt"),
        ];
    });
    assert_eq!(
        issues,
        [
            error("services/blocking/custom/1/name"),
            error("services/blocking/custom/2/url"),
            error("services/blocking/custom/3/url"),
            error("services/blocking/custom/4/url"),
        ]
    );
}

#[test]
fn domains_are_lowercase_names_once_each_and_never_both_allowed_and_blocked() {
    let issues = issues_after(|b| {
        b.allow = vec![
            "ok.example.com".into(),
            "Bad.example.com".into(),
            "zip".into(),
        ];
        b.deny = vec![
            "ok.example.com".into(),
            "a b".into(),
            "ads.example".into(),
            "ads.example".into(),
        ];
    });
    assert_eq!(
        issues,
        [
            error("services/blocking/allow/1"),
            error("services/blocking/deny/1"),
            error("services/blocking/deny/3"),
            error("services/blocking/deny/0"),
        ]
    );
}

#[test]
fn bypassed_clients_are_unicast_once_each() {
    let issues = issues_after(|b| {
        b.bypass = vec![
            "02:00:00:00:10:01".parse().unwrap(),
            "01:00:5e:00:00:01".parse().unwrap(),
            "02:00:00:00:10:01".parse().unwrap(),
        ];
    });
    assert_eq!(
        issues,
        [
            error("services/blocking/bypass/1"),
            error("services/blocking/bypass/2")
        ]
    );
}

#[test]
fn blocking_nothing_is_a_warning_only_while_on() {
    let none = |b: &mut Blocking| {
        b.lists.clear();
        b.custom.clear();
        b.deny.clear();
    };
    assert_eq!(
        issues_after(none),
        [("services/blocking/lists".to_owned(), Severity::Warning)]
    );
    assert_eq!(
        issues_after(|b| {
            none(b);
            b.enabled = false;
        }),
        []
    );
}
