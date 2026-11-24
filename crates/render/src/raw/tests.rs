//! Raw sections merged into rendered packages and patched onto the node's own files.

use std::collections::BTreeMap;

use uniwrt_model::site::{
    SecretRef,
    node::{RawSection, RawValue},
};
use uniwrt_uci::{Package, Section};

use super::{Merge, merge};
use crate::Issue;

/// Merges `raw` into `package` with the vault holding `secrets`; the warnings and errors.
fn merge_with(
    package: &mut Package,
    raw: &[RawSection],
    secrets: &BTreeMap<SecretRef, String>,
) -> (Vec<Issue>, Vec<Issue>) {
    let (mut warnings, mut errors) = (Vec::new(), Vec::new());
    let at = Merge {
        secrets,
        warnings: &mut warnings,
        errors: &mut errors,
        patch: false,
    };
    merge(package, raw, at);
    (warnings, errors)
}

fn raw(name: Option<&str>, kind: &str, options: &[(&str, RawValue)]) -> RawSection {
    RawSection {
        package: "network".to_owned(),
        kind: kind.to_owned(),
        name: name.map(str::to_owned),
        options: options
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect::<BTreeMap<_, _>>(),
        merge: false,
        unset: std::collections::BTreeSet::new(),
        remove: false,
        matching: std::collections::BTreeMap::new(),
    }
}

fn one(value: &str) -> RawValue {
    RawValue::Option(value.to_owned())
}

#[test]
fn raw_sections_replace_by_name_or_join() {
    let mut package = Package::new("network");
    package.sections = vec![
        Section::named("interface", "lan").with("proto", "static"),
        Section::named("interface", "wan").with("proto", "dhcp"),
    ];
    let mut other = raw(None, "rule", &[]);
    other.package = "firewall".to_owned();
    let (warnings, errors) = merge_with(
        &mut package,
        &[
            raw(Some("wan"), "interface", &[("proto", one("pppoe"))]),
            raw(
                None,
                "route",
                &[("target", RawValue::List(vec!["10.9.0.0/16".to_owned()]))],
            ),
            other,
        ],
        &BTreeMap::new(),
    );
    assert_eq!(package.sections.len(), 3);
    assert_eq!(package.sections[1].option("proto"), Some("pppoe"));
    assert_eq!(package.sections[2].kind, "route");
    assert_eq!((warnings, errors), (Vec::new(), Vec::new()));
}

#[test]
fn sealed_values_are_written_as_the_vault_holds_them() {
    let sealed = |c: char| SecretRef::new(format!("config/{}", c.to_string().repeat(32)));
    let (known, lost) = (sealed('a').unwrap(), sealed('b').unwrap());
    let secrets = BTreeMap::from([(known.clone(), "c2VjcmV0".to_owned())]);
    let mut package = Package::new("network");
    let (_, errors) = merge_with(
        &mut package,
        &[
            raw(
                Some("wg0"),
                "interface",
                &[
                    ("proto", one("wireguard")),
                    ("private_key", RawValue::Secret { secret: known }),
                ],
            ),
            raw(
                None,
                "wireguard_wg0",
                &[("preshared_key", RawValue::Secret { secret: lost })],
            ),
        ],
        &secrets,
    );
    assert_eq!(package.sections[0].option("private_key"), Some("c2VjcmV0"));
    assert_eq!(package.sections[1].option("preshared_key"), None);
    assert_eq!(
        errors.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(),
        ["node/raw/1/options/preshared_key"]
    );
}

#[test]
fn merges_set_options_on_the_section_they_name_or_key() {
    let mut package = Package::new("network");
    package.sections = vec![
        Section::named("interface", "lan")
            .with("proto", "static")
            .with("device", "br-lan"),
        Section::anonymous("device").with("name", "br-lan.10"),
        Section::anonymous("device")
            .with("name", "br-lan")
            .with("type", "bridge"),
    ];
    let merged = |mut raw: RawSection| {
        raw.merge = true;
        raw
    };
    let (warnings, _) = merge_with(
        &mut package,
        &[
            merged(raw(Some("lan"), "interface", &[("mtu", one("1400"))])),
            merged(raw(
                None,
                "device",
                &[("name", one("br-lan")), ("stp", one("1"))],
            )),
            merged(raw(
                None,
                "device",
                &[("name", one("eth9")), ("mtu", one("9000"))],
            )),
            merged(raw(None, "route", &[("target", one("10.9.0.0/16"))])),
        ],
        &BTreeMap::new(),
    );
    assert_eq!(package.sections.len(), 3, "a merge never adds a section");
    assert_eq!(package.sections[0].option("mtu"), Some("1400"));
    assert_eq!(package.sections[0].option("proto"), Some("static"));
    assert_eq!(package.sections[1].option("stp"), None);
    assert_eq!(package.sections[2].option("stp"), Some("1"));
    assert_eq!(
        warnings.iter().map(|w| w.path.as_str()).collect::<Vec<_>>(),
        ["node/raw/2", "node/raw/3"],
        "a merge without its section, or a type without a key, is left out"
    );
}

#[test]
fn removals_unsets_and_matches_find_their_section() {
    let mut package = Package::new("network");
    package.sections = vec![
        Section::anonymous("route").with("target", "10.1.0.0/16"),
        Section::anonymous("route")
            .with("target", "10.2.0.0/16")
            .with("metric", "5"),
        Section::named("interface", "guest").with("proto", "static"),
        Section::named("interface", "lan")
            .with("proto", "static")
            .with("ip6assign", "60"),
    ];
    let mut unset = raw(Some("lan"), "interface", &[("mtu", one("1400"))]);
    unset.merge = true;
    unset.unset.insert("ip6assign".to_owned());
    let mut matched = raw(None, "route", &[("metric", one("9"))]);
    matched.merge = true;
    matched
        .matching
        .insert("target".to_owned(), one("10.2.0.0/16"));
    let mut gone = raw(None, "route", &[]);
    gone.remove = true;
    gone.matching
        .insert("target".to_owned(), one("10.1.0.0/16"));
    let mut named = raw(Some("guest"), "interface", &[]);
    named.remove = true;
    let mut absent = raw(Some("nowhere"), "interface", &[]);
    absent.remove = true;
    let (warnings, errors) = merge_with(
        &mut package,
        &[unset, matched, gone, named, absent],
        &BTreeMap::new(),
    );
    assert_eq!((warnings, errors), (Vec::new(), Vec::new()));
    assert_eq!(package.sections.len(), 2);
    assert_eq!(package.sections[0].option("metric"), Some("9"));
    let lan = package.section("lan").unwrap();
    assert_eq!(lan.option("mtu"), Some("1400"));
    assert_eq!(lan.option("ip6assign"), None);
    assert_eq!(lan.option("proto"), Some("static"));
}

#[test]
fn a_patch_adds_an_anonymous_section_once() {
    let route = raw(None, "route", &[("target", one("10.9.0.0/16"))]);
    let mut package = Package::new("network");
    let secrets: BTreeMap<SecretRef, String> = BTreeMap::new();
    for _ in 0..2 {
        let (mut warnings, mut errors) = (Vec::new(), Vec::new());
        let at = Merge::new(&secrets, &mut warnings, &mut errors, true);
        merge(&mut package, std::slice::from_ref(&route), at);
    }
    assert_eq!(
        package.sections.len(),
        1,
        "the node's file holds it already"
    );
}
