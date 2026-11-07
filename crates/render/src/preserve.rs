//! Values the node generated for itself, carried over from its current packages: they are
//! the node's, not the site's, and losing them would change it. The DHCPv6 DUID (an ISP keys
//! the delegated prefix on it), the IPv6 ULA prefix while the site sets none (firstboot
//! generated it, and the LAN's ULA addresses depend on it), `compat_version` when board.json has
//! none (sysupgrade checks it), the board's LED sections, and odhcpd's file paths (they differ between releases).
//! Brownfield import (M3.5) keeps the rest a user added as raw sections.

use std::collections::BTreeMap;

use uniwrt_uci::{Package, Section};

/// Options of odhcpd's section that render owns; the others are carried over.
const ODHCPD_OWNED: [&str; 3] = ["maindhcp", "leasetrigger", "loglevel"];

pub(crate) fn carry_over(package: &mut Package, current: &BTreeMap<String, Package>) {
    let Some(now) = current.get(&package.name) else {
        return;
    };
    match package.name.as_str() {
        "network" => {
            copy_option(package, now, "globals", "dhcp_default_duid");
            if package
                .section("globals")
                .is_some_and(|globals| globals.get("ula_prefix").is_none())
            {
                copy_option(package, now, "globals", "ula_prefix");
            }
        }
        "system" => {
            if let (Some(system), Some(old)) = (
                first_mut(package, "system"),
                now.sections_of("system").next(),
            ) && system.get("compat_version").is_none()
                && let Some(value) = old.get("compat_version")
            {
                system.set("compat_version", value.clone());
            }
            package.sections.extend(now.sections_of("led").cloned());
        }
        "dhcp" => {
            if let (Some(odhcpd), Some(old)) =
                (package.section_mut("odhcpd"), now.section("odhcpd"))
            {
                for entry in &old.options {
                    if !ODHCPD_OWNED.contains(&entry.name.as_str())
                        && odhcpd.get(&entry.name).is_none()
                    {
                        odhcpd.set(&entry.name, entry.value.clone());
                    }
                }
            }
        }
        "upnpd" => copy_option(package, now, crate::upnp::CONFIG, "uuid"),
        _ => {}
    }
}

fn first_mut<'p>(package: &'p mut Package, kind: &str) -> Option<&'p mut Section> {
    package.sections.iter_mut().find(|s| s.kind == kind)
}

fn copy_option(package: &mut Package, from: &Package, section: &str, option: &str) {
    if let (Some(to), Some(value)) = (
        package.section_mut(section),
        from.section(section).and_then(|s| s.get(option)),
    ) {
        to.set(option, value.clone());
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use uniwrt_uci::{Package, Section};

    use super::carry_over;

    fn package(name: &str, sections: Vec<Section>) -> Package {
        let mut package = Package::new(name);
        package.sections = sections;
        package
    }

    #[test]
    fn node_values_carry_over() {
        let current = BTreeMap::from([
            (
                "network".to_owned(),
                package(
                    "network",
                    vec![
                        Section::named("globals", "globals")
                            .with("dhcp_default_duid", "0004ab")
                            .with("ula_prefix", "fd00:1::/48"),
                    ],
                ),
            ),
            (
                "system".to_owned(),
                package(
                    "system",
                    vec![
                        Section::anonymous("system").with("compat_version", "1.1"),
                        Section::anonymous("led").with("name", "WAN"),
                    ],
                ),
            ),
            (
                "dhcp".to_owned(),
                package(
                    "dhcp",
                    vec![
                        Section::named("odhcpd", "odhcpd")
                            .with("maindhcp", "1")
                            .with("piodir", "/tmp/odhcpd-piodir"),
                    ],
                ),
            ),
        ]);
        let mut network = package(
            "network",
            vec![Section::named("globals", "globals").with("ula_prefix", "fd00:2::/48")],
        );
        carry_over(&mut network, &current);
        let globals = network.section("globals").unwrap();
        assert_eq!(globals.option("dhcp_default_duid"), Some("0004ab"));
        assert_eq!(
            globals.option("ula_prefix"),
            Some("fd00:2::/48"),
            "the site's ULA wins"
        );

        let mut network = package("network", vec![Section::named("globals", "globals")]);
        carry_over(&mut network, &current);
        assert_eq!(
            network.section("globals").unwrap().option("ula_prefix"),
            Some("fd00:1::/48"),
            "the node's ULA stays while the site sets none"
        );

        let mut system = package("system", vec![Section::anonymous("system")]);
        carry_over(&mut system, &current);
        assert_eq!(system.sections[0].option("compat_version"), Some("1.1"));
        assert_eq!(system.sections[1].kind, "led");

        let mut dhcp = package(
            "dhcp",
            vec![Section::named("odhcpd", "odhcpd").with("maindhcp", "0")],
        );
        carry_over(&mut dhcp, &current);
        let odhcpd = dhcp.section("odhcpd").unwrap();
        assert_eq!(odhcpd.option("maindhcp"), Some("0"), "render owns maindhcp");
        assert_eq!(odhcpd.option("piodir"), Some("/tmp/odhcpd-piodir"));
    }
}
