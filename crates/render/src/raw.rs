//! A node's raw sections (the validated escape hatch, D-011): each joins its package after
//! everything rendered, or replaces the rendered section of the same name.

use uniwrt_model::site::node::{RawSection, RawValue};
use uniwrt_uci::{Package, Section, Value};

pub(crate) fn merge(package: &mut Package, raw: &[RawSection]) {
    for raw in raw.iter().filter(|r| r.package == package.name) {
        let mut section = match &raw.name {
            Some(name) => Section::named(raw.kind.as_str(), name.as_str()),
            None => Section::anonymous(raw.kind.as_str()),
        };
        for (name, value) in &raw.options {
            let value = match value {
                RawValue::Option(value) => Value::Single(value.clone()),
                RawValue::List(items) => Value::List(items.clone()),
            };
            section.set(name, value);
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use uniwrt_model::site::node::{RawSection, RawValue};
    use uniwrt_uci::{Package, Section};

    use super::merge;

    #[test]
    fn raw_sections_replace_by_name_or_join() {
        let mut package = Package::new("network");
        package.sections = vec![
            Section::named("interface", "lan").with("proto", "static"),
            Section::named("interface", "wan").with("proto", "dhcp"),
        ];
        let raw = |name: Option<&str>, kind: &str, options: &[(&str, RawValue)]| RawSection {
            package: "network".to_owned(),
            kind: kind.to_owned(),
            name: name.map(str::to_owned),
            options: options
                .iter()
                .map(|(k, v)| ((*k).to_owned(), v.clone()))
                .collect::<BTreeMap<_, _>>(),
        };
        let mut other = raw(None, "rule", &[]);
        other.package = "firewall".to_owned();
        merge(
            &mut package,
            &[
                raw(
                    Some("wan"),
                    "interface",
                    &[("proto", RawValue::Option("pppoe".to_owned()))],
                ),
                raw(
                    None,
                    "route",
                    &[("target", RawValue::List(vec!["10.9.0.0/16".to_owned()]))],
                ),
                other,
            ],
        );
        assert_eq!(package.sections.len(), 3);
        assert_eq!(package.sections[1].option("proto"), Some("pppoe"));
        assert_eq!(package.sections[2].kind, "route");
    }
}
