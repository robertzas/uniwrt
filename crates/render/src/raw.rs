//! A node's raw sections (the validated escape hatch, D-011): each joins its package after
//! everything rendered, or replaces the rendered section of the same name; a merge (D-072)
//! sets (or unsets) its options on a rendered section instead, and a removal removes it, found
//! by name or, for an anonymous one, by the options it matches or the key options of its type
//! ([`key`]). A package render doesn't write is patched the same way: the node's own file with
//! its raw sections (D-143). A sealed value ([`RawValue::Secret`], D-073) is
//! written as the vault holds it; one the vault lacks keeps the node from rendering.

use uniwrt_model::site::node::{RawSection, RawValue};
use uniwrt_uci::{Package, Section, Value};

use crate::{Issue, Secrets};

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
        ("dhcp", "boot") => &["networkid"],
        ("dhcp", "dhcp") => &["interface"],
        ("firewall", "forwarding") => &["src", "dest"],
        ("firewall", "include") => &["path"],
        ("upnpd", "perm_rule") => &["comment"],
        ("https-dns-proxy", "https-dns-proxy") => &["listen_port"],
        _ => return None,
    })
}

/// The section of `package` that a section of `kind`, named `name`, with the `option` values
/// given stands for: the one of that name, or for an anonymous one the first of its type with
/// the same key options (for a type held once, its first section, named or not).
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
    if key.is_empty() {
        // A type held once is its first section, which render may name (the main dnsmasq
        // while a DNS filter bypass is on, D-133).
        return package.sections.iter().position(|s| s.kind == kind);
    }
    package.sections.iter().position(|s| {
        s.kind == kind
            && s.name.is_none()
            && key
                .iter()
                .all(|k| s.option(k).is_some() && s.option(k) == option(k))
    })
}

/// Where raw sections join a package: the vault's values of their secrets, the render's
/// issues, and whether the package is the node's own file, patched.
pub(crate) struct Merge<'a> {
    pub(crate) secrets: &'a dyn Secrets,
    pub(crate) warnings: &'a mut Vec<Issue>,
    pub(crate) errors: &'a mut Vec<Issue>,
    /// The package is the node's own file (D-143), which holds what earlier applies added: an
    /// anonymous section it holds already isn't added again.
    pub(crate) patch: bool,
}

impl<'a> Merge<'a> {
    pub(crate) fn new(
        secrets: &'a dyn Secrets,
        warnings: &'a mut Vec<Issue>,
        errors: &'a mut Vec<Issue>,
        patch: bool,
    ) -> Self {
        Self {
            secrets,
            warnings,
            errors,
            patch,
        }
    }
}

pub(crate) fn merge(package: &mut Package, raw: &[RawSection], at: Merge<'_>) {
    let Merge {
        secrets,
        warnings,
        errors,
        patch,
    } = at;
    for (i, raw) in raw.iter().enumerate() {
        if raw.package != package.name {
            continue;
        }
        if raw.remove {
            // Nothing to remove is what a removal wants.
            if let Some(index) = target(package, raw) {
                package.sections.remove(index);
            }
            continue;
        }
        let mut values = Vec::with_capacity(raw.options.len());
        for (name, value) in &raw.options {
            match value_of(value, secrets) {
                Some(value) => values.push((name, value)),
                None => errors.push(Issue::new(
                    format!("node/raw/{i}/options/{name}"),
                    format!(
                        "The vault lacks the secret of {}'s {name}: set it again",
                        raw.package
                    ),
                )),
            }
        }
        if raw.merge {
            match target(package, raw) {
                Some(index) => {
                    let section = &mut package.sections[index];
                    for (name, value) in values {
                        section.set(name, value);
                    }
                    for name in &raw.unset {
                        section.remove(name);
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
        for (name, value) in values {
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
            None if patch && package.sections.contains(&section) => {}
            None => package.sections.push(section),
        }
    }
}

/// The section a merge or removal of `raw` finds: by name, by the options it matches, or by its
/// type's key ([`find`]).
fn target(package: &Package, raw: &RawSection) -> Option<usize> {
    if raw.name.is_none() && !raw.matching.is_empty() {
        return package.sections.iter().position(|s| {
            s.kind == raw.kind
                && s.name.is_none()
                && raw
                    .matching
                    .iter()
                    .all(|(name, value)| holds(s, name, value))
        });
    }
    find(package, &raw.kind, raw.name.as_deref(), |k| {
        match raw.options.get(k) {
            Some(RawValue::Option(value)) => Some(value.as_str()),
            _ => None,
        }
    })
}

/// Whether `section` holds `name` as `value` (a plain one: a secret never matches).
fn holds(section: &Section, name: &str, value: &RawValue) -> bool {
    match (section.get(name), value) {
        (Some(Value::Single(held)), RawValue::Option(value)) => held == value,
        (Some(Value::List(held)), RawValue::List(value)) => held == value,
        _ => false,
    }
}

fn value_of(value: &RawValue, secrets: &dyn Secrets) -> Option<Value> {
    Some(match value {
        RawValue::Option(value) => Value::Single(value.clone()),
        RawValue::List(items) => Value::List(items.clone()),
        RawValue::Secret { secret } => Value::Single(secrets.get(secret)?.to_owned()),
    })
}

#[cfg(test)]
mod tests;
