//! Reading a node's packages as OpenWrt does: booleans, lists written as options, addresses
//! with or without a netmask; and the renames that render's naming needs.

use std::{
    collections::{BTreeMap, BTreeSet},
    net::Ipv4Addr,
};

use uniwrt_core::Ipv4Net;
use uniwrt_model::site::validate::is_id;
use uniwrt_uci::{Package, Section};

/// Names on the nodes that render gives differently.
#[derive(Debug, Default)]
pub(crate) struct Renames {
    /// The gateway's LAN interfaces → the ids of the networks they became.
    pub interfaces: BTreeMap<String, String>,
    /// The gateway's firewall zones → the names render gives them (their network's id).
    pub zones: BTreeMap<String, String>,
    /// Sections the model can't hold although render writes one of that name, by package and
    /// name: they are kept whole (a WAN over a protocol the model lacks).
    pub whole: BTreeSet<(String, String)>,
}

impl Renames {
    /// The network id an interface became, if it is one.
    pub(crate) fn network<'a>(&'a self, interface: &'a str) -> &'a str {
        self.interfaces
            .get(interface)
            .map_or(interface, String::as_str)
    }
}

/// The first section of `kind` in `package`.
pub(crate) fn first<'p>(
    packages: &'p BTreeMap<String, Package>,
    package: &str,
    kind: &'p str,
) -> Option<&'p Section> {
    sections(packages, package, kind).next()
}

/// The sections of `kind` in `package`.
pub(crate) fn sections<'p>(
    packages: &'p BTreeMap<String, Package>,
    package: &str,
    kind: &'p str,
) -> impl Iterator<Item = &'p Section> + 'p {
    packages
        .get(package)
        .into_iter()
        .flat_map(move |p| p.sections_of(kind))
}

/// The items of a list, or of an option that holds several separated by spaces (as
/// `option network 'lan guest'`, which netifd and fw4 read as a list).
pub(crate) fn items<'s>(section: &'s Section, name: &str) -> Vec<&'s str> {
    match section.get(name) {
        Some(uniwrt_uci::Value::List(items)) => items.iter().map(String::as_str).collect(),
        Some(uniwrt_uci::Value::Single(value)) => value.split_whitespace().collect(),
        None => Vec::new(),
    }
}

/// A UCI boolean, as `config_get_bool` reads it.
pub(crate) fn flag(section: &Section, name: &str) -> Option<bool> {
    match section.option(name)? {
        "1" | "on" | "true" | "yes" | "enabled" => Some(true),
        "0" | "off" | "false" | "no" | "disabled" => Some(false),
        _ => None,
    }
}

/// The first IPv4 address with its prefix: `ipaddr` in CIDR form, or with `netmask`.
pub(crate) fn ipv4(section: &Section) -> Option<Ipv4Net> {
    let first = *items(section, "ipaddr").first()?;
    if let Ok(net) = first.parse::<Ipv4Net>() {
        return Some(net);
    }
    let addr: Ipv4Addr = first.parse().ok()?;
    let mask: Ipv4Addr = section.option("netmask")?.parse().ok()?;
    let bits = u32::from(mask);
    let len = bits.leading_ones();
    (bits.checked_shl(len).unwrap_or(0) == 0)
        .then(|| Ipv4Net::new(addr, u8::try_from(len).ok()?).ok())
        .flatten()
}

/// An id (`[a-z][a-z0-9_]{0,11}`) made from `text`, unique among those `taken` says are.
pub(crate) fn id(text: &str, fallback: &str, taken: impl Fn(&str) -> bool) -> String {
    let mut base: String = text
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    base = base.trim_matches('_').to_owned();
    if !base.starts_with(|c: char| c.is_ascii_lowercase()) {
        base = format!("{fallback}{base}");
    }
    base.truncate(12);
    let base = base.trim_end_matches('_').to_owned();
    if is_id(&base) && !taken(&base) {
        return base;
    }
    (2..1000)
        .map(|n| {
            let suffix = format!("_{n}");
            let mut stem = base.clone();
            stem.truncate(12 - suffix.len());
            format!("{stem}{suffix}")
        })
        .find(|candidate| is_id(candidate) && !taken(candidate))
        .unwrap_or_else(|| fallback.to_owned())
}

#[cfg(test)]
mod tests {
    use uniwrt_uci::Section;

    use super::{flag, id, ipv4, items};

    #[test]
    fn values_read_as_openwrt_reads_them() {
        let section = Section::named("interface", "lan")
            .with("ipaddr", "192.168.1.1")
            .with("netmask", "255.255.255.0")
            .with("network", "lan guest")
            .with("auto", "on");
        assert_eq!(ipv4(&section).unwrap().to_string(), "192.168.1.1/24");
        assert_eq!(items(&section, "network"), ["lan", "guest"]);
        assert_eq!(flag(&section, "auto"), Some(true));
        let cidr = Section::named("interface", "lan").with("ipaddr", ["10.0.0.1/16"]);
        assert_eq!(ipv4(&cidr).unwrap().to_string(), "10.0.0.1/16");
        let holes = Section::named("interface", "lan")
            .with("ipaddr", "10.0.0.1")
            .with("netmask", "255.0.255.0");
        assert_eq!(ipv4(&holes), None);
    }

    #[test]
    fn ids_are_made_unique_and_valid() {
        assert_eq!(id("Home Wi-Fi", "w", |_| false), "home_wi_fi");
        assert_eq!(id("5G Network", "w", |_| false), "w5g_network");
        assert_eq!(id("home", "w", |t| t == "home"), "home_2");
        assert_eq!(
            id("A very long network name", "n", |_| false),
            "a_very_long"
        );
        assert_eq!(id("???", "w", |_| false), "w");
    }
}
