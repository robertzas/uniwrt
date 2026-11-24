//! Per-node overrides: hostnames, radios, port profiles and raw UCI sections.

use std::collections::BTreeSet;

use uniwrt_core::NodeId;

use super::{Issues, names, wireless::tx_power};
use crate::site::{
    SiteConfig,
    node::{NodeConfig, RawValue, is_secret_option},
};

pub(super) fn nodes(config: &SiteConfig, adopted: &BTreeSet<NodeId>, issues: &mut Issues) {
    let mut seen = BTreeSet::new();
    for node in &config.nodes {
        let at = format!("nodes/{}", node.node);
        if !seen.insert(node.node) {
            issues.error(&at, format!("Node {} has two entries", node.node));
        }
        if !adopted.contains(&node.node) {
            issues.warn(&at, format!("Node {} is not adopted", node.node));
        }
        if let Some(hostname) = &node.hostname
            && !names::is_hostname(hostname)
        {
            issues.error(
                format!("{at}/hostname"),
                format!("{hostname:?} is not a host name: letters, digits and hyphens"),
            );
        }
        radios(node, &at, issues);
        super::ports::ports(config, node, &at, issues);
        raw(node, &at, issues);
    }
}

fn radios(node: &NodeConfig, at: &str, issues: &mut Issues) {
    let mut seen = BTreeSet::new();
    for radio in &node.radios {
        let at = format!("{at}/radios/{}", radio.radio);
        if !uniwrt_uci::is_name(&radio.radio) {
            issues.error(&at, format!("{:?} is not a radio's UCI name", radio.radio));
        }
        if !seen.insert(radio.radio.as_str()) {
            issues.error(&at, format!("Radio {} has two overrides", radio.radio));
        }
        if let Some(power) = radio.tx_power {
            tx_power(power, &format!("{at}/tx_power"), issues);
        }
    }
}

/// A raw section's name to check: the field, its value if any, and the rule.
type Check<'a> = (&'a str, Option<&'a String>, fn(&str) -> bool);

fn raw(node: &NodeConfig, at: &str, issues: &mut Issues) {
    for (i, section) in node.raw.iter().enumerate() {
        let at = format!("{at}/raw/{i}");
        let checks: [Check<'_>; 3] = [
            ("package", Some(&section.package), uniwrt_uci::is_package),
            ("type", Some(&section.kind), uniwrt_uci::is_type),
            ("name", section.name.as_ref(), uniwrt_uci::is_name),
        ];
        for (field, name, valid) in checks {
            if let Some(name) = name
                && !valid(name)
            {
                issues.error(
                    format!("{at}/{field}"),
                    format!("{name:?} is not a valid UCI {field}"),
                );
            }
        }
        if section.merge && section.remove {
            issues.error(
                &at,
                "A raw section either merges into its section or removes it",
            );
        }
        if section.remove && section.name.is_some() && !section.options.is_empty() {
            issues.error(
                format!("{at}/options"),
                "Removing a named section needs no options",
            );
        }
        for option in &section.unset {
            if !section.merge {
                issues.error(
                    format!("{at}/unset/{option}"),
                    "Only a merge removes options",
                );
            } else if !uniwrt_uci::is_name(option) {
                issues.error(
                    format!("{at}/unset/{option}"),
                    format!("{option:?} is not a UCI option name"),
                );
            } else if section.options.contains_key(option) {
                issues.error(
                    format!("{at}/unset/{option}"),
                    format!("{option} is both set and removed"),
                );
            }
        }
        if !section.matching.is_empty() && (section.name.is_some() || !section.finds()) {
            issues.error(
                format!("{at}/matching"),
                "Only a merge or a removal of an anonymous section finds it by its options",
            );
        }
        for (option, value) in &section.matching {
            let at = format!("{at}/matching/{option}");
            if is_secret_option(option) || matches!(value, RawValue::Secret { .. }) {
                issues.error(&at, "A secret never identifies a section");
            }
            option_value(option, value, &at, issues);
        }
        for (option, value) in &section.options {
            option_value(option, value, &format!("{at}/options/{option}"), issues);
        }
    }
}

/// Checks an option's name and value at `at`.
fn option_value(option: &str, value: &RawValue, at: &str, issues: &mut Issues) {
    if !uniwrt_uci::is_name(option) {
        issues.error(at, format!("{option:?} is not a UCI option name"));
    }
    let values = match value {
        RawValue::Option(value) => std::slice::from_ref(value),
        RawValue::List(values) => values.as_slice(),
        RawValue::Secret { .. } => &[],
    };
    if values
        .iter()
        .any(|value| value.contains(['\0', '\n', '\r']))
    {
        issues.error(at, "A value holds no line breaks or NUL characters");
    }
}
