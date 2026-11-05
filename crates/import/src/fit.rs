//! Fitting one node to the import (D-072): its sections renamed as render names them, paired
//! with the rendered ones, and what the model doesn't hold kept raw, so that the import renders
//! as the node is, up to the residual: the options render writes its own way.

use std::collections::BTreeMap;

use uniwrt_model::{
    node::NodeRole,
    site::{
        SecretRef, SiteConfig,
        node::{ManagedPackage, RawSection, RawValue},
    },
};
use uniwrt_render::{MESH_IFACE, RENDERABLE, Rendered, Target, raw};
use uniwrt_uci::{Package, Section, Value};

use crate::{Fate, NodeImport, SectionImport, Source, node_config, read::Renames};

pub(crate) fn node(
    site: &mut SiteConfig,
    secrets: &BTreeMap<SecretRef, String>,
    source: &Source<'_>,
    renames: &Renames,
) -> NodeImport {
    let mut original: BTreeMap<ManagedPackage, Package> = RENDERABLE
        .iter()
        .map(|&p| {
            let package = source
                .packages
                .get(p.name())
                .cloned()
                .unwrap_or_else(|| Package::new(p.name()));
            (p, package)
        })
        .collect();
    let names: BTreeMap<ManagedPackage, Vec<Option<String>>> = original
        .iter()
        .map(|(p, package)| {
            (
                *p,
                package.sections.iter().map(|s| s.name.clone()).collect(),
            )
        })
        .collect();
    rename_interfaces(&mut original, &renames.interfaces);
    if source.role == NodeRole::Gateway
        && let Some(firewall) = original.get_mut(&ManagedPackage::Firewall)
    {
        rename_zones(firewall, &renames.zones);
    }
    node_config(site, source.node).raw.clear();
    let failed = |issues| NodeImport {
        node: source.node,
        sections: Vec::new(),
        residual: Vec::new(),
        issues,
        warnings: Vec::new(),
        confidence: 0,
    };
    let rendered = match trial(site, secrets, source) {
        Ok(rendered) => rendered,
        Err(issues) => return failed(issues),
    };
    rename_wifi(&mut original, &rendered);
    let (sections, raws, confidence) =
        classify(&mut original, &names, &rendered, renames, source.role);
    node_config(site, source.node).raw = raws;
    let rendered = match trial(site, secrets, source) {
        Ok(rendered) => rendered,
        Err(issues) => return failed(issues),
    };
    let residual = original
        .iter()
        .filter_map(|(package, before)| {
            let diff = uniwrt_uci::diff(before, rendered.packages.get(package)?);
            (!diff.is_empty()).then_some(diff)
        })
        .collect();
    NodeImport {
        node: source.node,
        sections,
        residual,
        issues: Vec::new(),
        warnings: rendered.warnings,
        confidence,
    }
}

/// Each section's fate, the raw sections that keep what the model doesn't hold, and the
/// confidence: the share of options the model holds.
fn classify(
    original: &mut BTreeMap<ManagedPackage, Package>,
    names: &BTreeMap<ManagedPackage, Vec<Option<String>>>,
    rendered: &Rendered,
    renames: &Renames,
    role: NodeRole,
) -> (Vec<SectionImport>, Vec<RawSection>, u8) {
    let ap_firewall = role != NodeRole::Gateway;
    let mut sections = Vec::new();
    let mut raws = Vec::new();
    let (mut total, mut modeled) = (0_usize, 0_usize);
    // The interfaces kept whole, which the zones that hold them keep holding.
    let mut kept_interfaces: Vec<String> = Vec::new();
    for (package, before) in original {
        let Some(after) = rendered.packages.get(package) else {
            continue;
        };
        let pairs = pair(
            before,
            after,
            renames,
            ap_firewall && *package == ManagedPackage::Firewall,
        );
        // Merges in the rendered order (where they land doesn't matter, so a re-import finds
        // them alike), then the whole sections in the node's order.
        let mut merges = Vec::new();
        let mut joins = Vec::new();
        for (index, (section, paired)) in before.sections.iter_mut().zip(pairs).enumerate() {
            let weight = section.options.len().max(1);
            total += weight;
            let fate = match paired {
                Pairing::With(at) => {
                    let target = &after.sections[at];
                    same_meaning(section, target);
                    let mut kept: Vec<uniwrt_uci::Entry> = section
                        .options
                        .iter()
                        .filter(|e| {
                            target.get(&e.name).is_none()
                                && !consumed(&before.name, &section.kind, &e.name)
                                && keepable(e)
                        })
                        .cloned()
                        .collect();
                    if *package == ManagedPackage::Firewall && section.kind == "zone" {
                        kept.extend(zone_members(section, target, &kept_interfaces));
                    }
                    modeled += weight
                        .saturating_sub(kept.len())
                        .max(usize::from(kept.is_empty()));
                    if !kept.is_empty() {
                        merges.push((at, merge(&after.name, target, &kept)));
                    }
                    Fate::Mapped {
                        kept: kept.iter().map(|e| e.name.clone()).collect(),
                    }
                }
                Pairing::Alone => {
                    if *package == ManagedPackage::Network
                        && section.kind == "interface"
                        && let Some(name) = &section.name
                    {
                        kept_interfaces.push(name.clone());
                    }
                    joins.push(whole(&before.name, section));
                    Fate::Raw
                }
                Pairing::Dropped => {
                    modeled += weight;
                    Fate::Dropped
                }
            };
            let name = names[package].get(index).cloned().flatten();
            sections.push(SectionImport {
                package: before.name.clone(),
                kind: section.kind.clone(),
                renamed: (section.name != name)
                    .then(|| section.name.clone())
                    .flatten(),
                name,
                fate,
            });
        }
        merges.sort_by_key(|(at, _)| *at);
        raws.extend(merges.into_iter().map(|(_, raw)| raw));
        raws.append(&mut joins);
    }
    let confidence = (modeled * 100)
        .checked_div(total)
        .map_or(100, |c| u8::try_from(c).unwrap_or(100));
    (sections, raws, confidence)
}

/// Renders the node with every package render writes managed.
fn trial(
    site: &SiteConfig,
    secrets: &BTreeMap<SecretRef, String>,
    source: &Source<'_>,
) -> Result<Rendered, Vec<uniwrt_render::Issue>> {
    let mut trial = site.clone();
    node_config(&mut trial, source.node).managed = RENDERABLE.into_iter().collect();
    uniwrt_render::render(
        &trial,
        secrets,
        Target {
            node: source.node,
            role: source.role,
            facts: source.facts,
            current: source.packages,
        },
    )
}

enum Pairing {
    /// The rendered section at this index stands for it.
    With(usize),
    Alone,
    Dropped,
}

/// Pairs each section with an unpaired rendered one: of the same name and type, or of the
/// same type and key options ([`raw::key`]; MAC addresses in either case).
fn pair(before: &Package, after: &Package, renames: &Renames, drop: bool) -> Vec<Pairing> {
    let mut used = vec![false; after.sections.len()];
    let mut pairs = Vec::new();
    for section in &before.sections {
        let name = section.name.as_deref();
        if name.is_some_and(|n| renames.whole.contains(&(before.name.clone(), n.to_owned()))) {
            pairs.push(Pairing::Alone);
            continue;
        }
        if drop && section.kind != "defaults" {
            pairs.push(Pairing::Dropped);
            continue;
        }
        let candidates = || {
            after
                .sections
                .iter()
                .enumerate()
                .filter(|(i, s)| !used[*i] && s.kind == section.kind)
        };
        let by_name = name.and_then(|n| {
            candidates()
                .find(|(_, s)| s.name.as_deref() == Some(n))
                .map(|(i, _)| i)
        });
        let by_key = || {
            let key = raw::key(&before.name, &section.kind)?;
            candidates()
                .find(|(_, s)| {
                    key.iter().all(|k| match (section.option(k), s.option(k)) {
                        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
                        _ => false,
                    })
                })
                .map(|(i, _)| i)
        };
        match by_name.or_else(by_key) {
            Some(index) => {
                used[index] = true;
                pairs.push(Pairing::With(index));
            }
            None => pairs.push(Pairing::Alone),
        }
    }
    pairs
}

/// A zone's `network` as render writes it plus the node's interfaces that were kept whole (a
/// 6rd tunnel in the WAN's zone), so they stay in their zone.
fn zone_members(section: &Section, target: &Section, kept: &[String]) -> Option<uniwrt_uci::Entry> {
    let rendered = crate::read::items(target, "network");
    let extra: Vec<&str> = crate::read::items(section, "network")
        .into_iter()
        .filter(|i| !rendered.contains(i) && kept.iter().any(|k| k == i))
        .collect();
    if extra.is_empty() {
        return None;
    }
    let items = rendered
        .iter()
        .chain(&extra)
        .map(|i| (*i).to_owned())
        .collect();
    Some(uniwrt_uci::Entry {
        name: "network".to_owned(),
        value: Value::List(items),
    })
}

/// Writes the options that mean what render writes as render writes them (an address or prefix
/// in another form, a boolean as `on`), so that only real changes are the residual.
fn same_meaning(section: &mut Section, target: &Section) {
    for entry in &mut section.options {
        let Some(rendered) = target.get(&entry.name) else {
            continue;
        };
        let (a, b) = (entry.value.items(), rendered.items());
        if entry.value != *rendered
            && a.len() == b.len()
            && a.iter().zip(b).all(|(a, b)| equivalent(a, b))
        {
            entry.value = rendered.clone();
        }
    }
}

fn equivalent(a: &str, b: &str) -> bool {
    use std::net::IpAddr;

    use uniwrt_core::IpNet;
    let flag = |v: &str| match v {
        "1" | "on" | "true" | "yes" | "enabled" => Some(true),
        "0" | "off" | "false" | "no" | "disabled" => Some(false),
        _ => None,
    };
    a == b
        || matches!((a.parse::<IpNet>(), b.parse::<IpNet>()), (Ok(x), Ok(y)) if x == y)
        || matches!((a.parse::<IpAddr>(), b.parse::<IpAddr>()), (Ok(x), Ok(y)) if x == y)
        || matches!((flag(a), flag(b)), (Some(x), Some(y)) if x == y)
}

/// Whether the model holds an option that render writes in another form (`netmask`, which
/// render folds into `ipaddr`): it is neither kept nor lost.
pub(crate) fn consumed(package: &str, kind: &str, option: &str) -> bool {
    matches!((package, kind, option), ("network", "interface", "netmask"))
}

/// Whether a raw section can hold the option (its name and value pass validation).
fn keepable(entry: &uniwrt_uci::Entry) -> bool {
    uniwrt_uci::is_name(&entry.name)
        && entry
            .value
            .items()
            .iter()
            .all(|v| !v.contains(['\0', '\n', '\r']))
}

fn raw_value(value: &Value) -> RawValue {
    match value {
        Value::Single(value) => RawValue::Option(value.clone()),
        Value::List(items) => RawValue::List(items.clone()),
    }
}

/// The options of a mapped section render doesn't write, as a merge onto `target`.
fn merge(package: &str, target: &Section, kept: &[uniwrt_uci::Entry]) -> RawSection {
    let mut options: BTreeMap<String, RawValue> = kept
        .iter()
        .map(|e| (e.name.clone(), raw_value(&e.value)))
        .collect();
    if target.name.is_none() {
        for k in raw::key(package, &target.kind).unwrap_or_default() {
            if let Some(value) = target.get(k) {
                options.insert((*k).to_owned(), raw_value(value));
            }
        }
    }
    RawSection {
        package: package.to_owned(),
        kind: target.kind.clone(),
        name: target.name.clone(),
        options,
        merge: true,
    }
}

/// A section the model doesn't hold, kept whole.
fn whole(package: &str, section: &Section) -> RawSection {
    RawSection {
        package: package.to_owned(),
        kind: section.kind.clone(),
        name: section.name.clone(),
        options: section
            .options
            .iter()
            .filter(|e| keepable(e))
            .map(|e| (e.name.clone(), raw_value(&e.value)))
            .collect(),
        merge: false,
    }
}

/// Renames the gateway's LAN interfaces to their networks' ids, wherever they are named.
fn rename_interfaces(
    packages: &mut BTreeMap<ManagedPackage, Package>,
    map: &BTreeMap<String, String>,
) {
    if map.is_empty() {
        return;
    }
    let rename = |name: &mut Option<String>| {
        if let Some(new) = name.as_deref().and_then(|n| map.get(n)) {
            *name = Some(new.clone());
        }
    };
    for (package, content) in packages.iter_mut() {
        for section in &mut content.sections {
            match (package, section.kind.as_str()) {
                (ManagedPackage::Network, "interface") => rename(&mut section.name),
                (ManagedPackage::Dhcp, "dhcp") => {
                    rename(&mut section.name);
                    replace_items(section, "interface", |i| map.get(i).cloned());
                }
                (ManagedPackage::Wireless, "wifi-iface") | (ManagedPackage::Firewall, "zone") => {
                    replace_items(section, "network", |i| map.get(i).cloned());
                }
                _ => {}
            }
        }
    }
}

/// Renames the gateway's zones as render names them, wherever they are named.
fn rename_zones(firewall: &mut Package, map: &BTreeMap<String, String>) {
    for section in &mut firewall.sections {
        let options: &[&str] = match section.kind.as_str() {
            "zone" => &["name"],
            "forwarding" | "rule" | "redirect" | "nat" => &["src", "dest"],
            _ => &[],
        };
        for option in options {
            replace_items(section, option, |z| map.get(z).cloned());
        }
    }
}

/// Replaces the items of an option or list that `new` maps.
fn replace_items(section: &mut Section, option: &str, new: impl Fn(&str) -> Option<String>) {
    let Some(value) = section.get(option).cloned() else {
        return;
    };
    let value = match value {
        Value::Single(v) => {
            let items: Vec<String> = v
                .split_whitespace()
                .map(|i| new(i).unwrap_or_else(|| i.to_owned()))
                .collect();
            Value::Single(items.join(" "))
        }
        Value::List(items) => Value::List(
            items
                .iter()
                .map(|i| new(i).unwrap_or_else(|| i.clone()))
                .collect(),
        ),
    };
    section.set(option, value);
}

/// Names the node's access points and mesh point as render does (`uw_<wlan>_<band>`,
/// `uw_mesh`), pairing them by radio and SSID, and the mesh point's batman-adv hard interface
/// `uw_mesh` too.
fn rename_wifi(original: &mut BTreeMap<ManagedPackage, Package>, rendered: &Rendered) {
    let Some(after) = rendered.packages.get(&ManagedPackage::Wireless) else {
        return;
    };
    let Some(wireless) = original.get_mut(&ManagedPackage::Wireless) else {
        return;
    };
    let mut used = vec![false; after.sections.len()];
    let mut hardif = None;
    for section in wireless
        .sections
        .iter_mut()
        .filter(|s| s.kind == "wifi-iface")
    {
        let mode = section.option("mode").unwrap_or("ap").to_owned();
        let found = after.sections.iter().enumerate().position(|(i, s)| {
            !used[i]
                && s.kind == "wifi-iface"
                && s.option("mode") == Some(mode.as_str())
                && match mode.as_str() {
                    "mesh" => true,
                    _ => {
                        s.option("device") == section.option("device")
                            && s.option("ssid") == section.option("ssid")
                    }
                }
        });
        let Some(index) = found else { continue };
        used[index] = true;
        section.name.clone_from(&after.sections[index].name);
        if mode == "mesh" {
            hardif = read_first(section, "network");
            section.set("network", MESH_IFACE);
        }
    }
    if let (Some(hardif), Some(network)) = (hardif, original.get_mut(&ManagedPackage::Network))
        && hardif != MESH_IFACE
        && let Some(section) = network.section_mut(&hardif)
    {
        section.name = Some(MESH_IFACE.to_owned());
    }
}

fn read_first(section: &Section, option: &str) -> Option<String> {
    crate::read::items(section, option)
        .first()
        .map(|s| (*s).to_owned())
}
