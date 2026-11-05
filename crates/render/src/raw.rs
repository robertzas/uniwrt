//! A node's raw sections (the validated escape hatch, D-011): each joins its package after
//! everything rendered, or replaces the rendered section of the same name; a merge (D-072)
//! sets its options on a rendered section instead, found by name or, for an anonymous one, by
//! the key options of its type ([`key`]).

use uniwrt_model::site::node::{RawSection, RawValue};
use uniwrt_uci::{Package, Section, Value};

use crate::Issue;

/// The options that identify an anonymous section of `kind` in `package`, as render writes
/// them: a merge, and brownfield import pairing a node's sections with rendered ones, find a
/// section by them. Empty for a type a package holds once (`dnsmasq`, `defaults`, `system`):
/// its first section. `None` for a type without an identity.
pub fn key(package: &str, kind: &str) -> Option<&'static [&'static str]> {
    Some(match (package, kind) {
        ("network", "device" | "switch")
        | ("dhcp", "domain")
        | ("firewall", "zone" | "rule" | "redirect") => &["name"],
        ("network", "bridge-vlan") => &["device", "vlan"],
        ("network", "switch_vlan") => &["device", "vid"],
        ("dhcp", "dnsmasq") | ("firewall", "defaults") | ("system", "system") => &[],
        ("dhcp", "host") => &["mac"],
        ("dhcp", "cname") => &["cname"],
        ("dhcp", "dhcp") => &["interface"],
        ("firewall", "forwarding") => &["src", "dest"],
        ("firewall", "include") => &["path"],
        _ => return None,
    })
}

/// The section of `package` that a section of `kind`, named `name`, with the `option` values
/// given stands for: the one of that name, or for an anonymous one the first of its type with
/// the same key options.
pub fn find<'v>(
    package: &Package,
    kind: &str,
    name: Option<&str>,
    option: impl Fn(&str) -> Option<&'v str>,
) -> Option<usize> {
    if let Some(name) = name {
        return package
            .sections
            .iter()
            .position(|s| s.name.as_deref() == Some(name));
    }
    let key = key(&package.name, kind)?;
    package.sections.iter().position(|s| {
        s.kind == kind
            && s.name.is_none()
            && key
                .iter()
                .all(|k| s.option(k).is_some() && s.option(k) == option(k))
    })
}

pub(crate) fn merge(package: &mut Package, raw: &[RawSection], warnings: &mut Vec<Issue>) {
    for (i, raw) in raw.iter().enumerate() {
        if raw.package != package.name {
            continue;
        }
        if raw.merge {
            let target = find(package, &raw.kind, raw.name.as_deref(), |k| {
                match raw.options.get(k) {
                    Some(RawValue::Option(value)) => Some(value.as_str()),
                    _ => None,
                }
            });
            match target {
                Some(index) => {
                    let section = &mut package.sections[index];
                    for (name, value) in &raw.options {
                        section.set(name, value_of(value));
                    }
                }
                None => warnings.push(Issue::new(
                    format!("node/raw/{i}"),
                    format!(
                        "The options kept for a {} {} no longer find their section: they are left out",
                        raw.package, raw.kind
                    ),
                )),
            }
            continue;
        }
        let mut section = match &raw.name {
            Some(name) => Section::named(raw.kind.as_str(), name.as_str()),
            None => Section::anonymous(raw.kind.as_str()),
        };
        for (name, value) in &raw.options {
            section.set(name, value_of(value));
        }
        let existing = raw.name.as_ref().and_then(|name| {
            package
                .sections
                .iter()
                .position(|s| s.name.as_ref() == Some(name))
        });
        match existing {
            Some(index) => package.sections[index] = section,
            None => package.sections.push(section),
        }
    }
}

fn value_of(value: &RawValue) -> Value {
    match value {
        RawValue::Option(value) => Value::Single(value.clone()),
        RawValue::List(items) => Value::List(items.clone()),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use uniwrt_model::site::node::{RawSection, RawValue};
    use uniwrt_uci::{Package, Section};

    use super::merge;

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
        let mut warnings = Vec::new();
        merge(
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
            &mut warnings,
        );
        assert_eq!(package.sections.len(), 3);
        assert_eq!(package.sections[1].option("proto"), Some("pppoe"));
        assert_eq!(package.sections[2].kind, "route");
        assert_eq!(warnings, Vec::<crate::Issue>::new());
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
        let mut warnings = Vec::new();
        merge(
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
            &mut warnings,
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
}
