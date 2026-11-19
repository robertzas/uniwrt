//! What a node's feeds offer, read once its index is refreshed ([`super::update`]): apk's
//! `query` over the available packages, or opkg's lists (`/var/opkg-lists/*`, gzipped) through
//! `grep`; and the installed packages with a newer version. Only the fields needed are sent.

use std::collections::BTreeMap;

use uniwrt_model::{
    facts::PackageManager,
    inventory::Package,
    packages::{AvailablePackage, PackageUpgrade},
};

/// opkg's lists, every feed's, through `grep`; a fixed script (no value is interpolated).
const OPKG_LISTS: &str = "for f in /var/opkg-lists/*; do case $f in *.sig) ;; *) gunzip -c \"$f\" ;; esac; done \
     | grep -E '^(Package|Version|Installed-Size|Description):'";

/// The command whose output [`parse_catalog`] reads.
pub fn catalog(manager: PackageManager) -> Vec<String> {
    let argv: &[&str] = match manager {
        PackageManager::Apk => &[
            "apk",
            "query",
            "--fields",
            "name,version,installed-size,description",
            "--available",
            "*",
        ],
        PackageManager::Opkg => &["sh", "-c", OPKG_LISTS],
    };
    argv.iter().map(|s| (*s).to_owned()).collect()
}

/// The command whose output [`parse_upgrades`] reads.
pub fn upgradable(manager: PackageManager) -> Vec<String> {
    let argv: &[&str] = match manager {
        PackageManager::Apk => &[
            "apk",
            "query",
            "--fields",
            "name,version",
            "--upgradable",
            "*",
        ],
        PackageManager::Opkg => &["opkg", "list-upgradable"],
    };
    argv.iter().map(|s| (*s).to_owned()).collect()
}

/// The command listing the firmware's own packages (`/rom`, on a squashfs image): it fails
/// where there is none, which reads as none.
pub fn rom(manager: PackageManager) -> Vec<String> {
    let argv: [&str; 4] = match manager {
        PackageManager::Apk => ["grep", "-E", "^P:", "/rom/lib/apk/db/installed"],
        PackageManager::Opkg => ["grep", "-E", "^Package:", "/rom/usr/lib/opkg/status"],
    };
    argv.map(str::to_owned).to_vec()
}

/// The names in the output of [`rom`].
pub fn parse_rom(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| {
            line.strip_prefix("P:")
                .or_else(|| line.strip_prefix("Package:"))
        })
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .collect()
}

/// The packages in the output of [`catalog`], by name, the first of each name kept. Both
/// managers write `Field: value` lines, a package's first field being its name.
pub fn parse_catalog(output: &str) -> Vec<AvailablePackage> {
    let mut found: BTreeMap<String, AvailablePackage> = BTreeMap::new();
    let mut current: Option<AvailablePackage> = None;
    let mut keep = |package: Option<AvailablePackage>| {
        if let Some(package) = package
            && !package.version.is_empty()
        {
            found.entry(package.name.clone()).or_insert(package);
        }
    };
    for line in output.lines() {
        let Some((field, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match field {
            "Name" | "Package" => {
                keep(current.take());
                current = Some(AvailablePackage {
                    name: value.to_owned(),
                    version: String::new(),
                    installed_bytes: None,
                    description: String::new(),
                });
            }
            "Version" => {
                if let Some(package) = &mut current {
                    value.clone_into(&mut package.version);
                }
            }
            "Installed-Size" => {
                if let Some(package) = &mut current {
                    package.installed_bytes = value.parse().ok();
                }
            }
            "Description" => {
                if let Some(package) = &mut current {
                    value.clone_into(&mut package.description);
                }
            }
            _ => {}
        }
    }
    keep(current);
    found.into_values().collect()
}

/// The upgrades in the output of [`upgradable`], by name: opkg's `name - from - to` lines,
/// or apk's names with their new version, `installed` giving the version they replace.
pub fn parse_upgrades(
    manager: PackageManager,
    output: &str,
    installed: &[Package],
) -> Vec<PackageUpgrade> {
    let mut upgrades: Vec<PackageUpgrade> = match manager {
        PackageManager::Opkg => output
            .lines()
            .filter_map(|line| {
                let mut parts = line.split(" - ");
                let (name, from, to) = (parts.next()?, parts.next()?, parts.next()?);
                Some(PackageUpgrade {
                    name: name.trim().to_owned(),
                    from: from.trim().to_owned(),
                    to: to.trim().to_owned(),
                })
            })
            .collect(),
        PackageManager::Apk => parse_catalog(output)
            .into_iter()
            .map(|package| PackageUpgrade {
                from: installed
                    .iter()
                    .find(|p| p.name == package.name)
                    .map_or_else(String::new, |p| p.version.clone()),
                name: package.name,
                to: package.version,
            })
            .collect(),
    };
    upgrades.retain(|upgrade| !upgrade.name.is_empty() && upgrade.from != upgrade.to);
    upgrades.sort_by(|a, b| a.name.cmp(&b.name));
    upgrades.dedup_by(|a, b| a.name == b.name);
    upgrades
}

#[cfg(test)]
mod tests {
    use uniwrt_model::{facts::PackageManager, inventory::Package};

    use super::*;

    #[test]
    fn apk_s_query_and_opkg_s_lists_read_alike() {
        // As 25.12's apk 3.0.5 and 24.10's lists write them.
        let apk = "Name: tcpdump\nVersion: 4.99.6-r1\nDescription: Network monitoring and data acquisition tool\n\
                   Installed-Size: 890772\n\nName: tcpdump-mini\nVersion: 4.99.6-r1\n\
                   Description: Network monitoring and data acquisition tool (minimal version)\n\
                   Installed-Size: 386940\n\nName: tcpdump\nVersion: 4.99.5-r1\n";
        let opkg = "Package: tcpdump\nVersion: 4.99.5-r1\nInstalled-Size: 901120\n\
                    Description:  Network monitoring and data acquisition tool\nPackage: terminfo\nVersion: 6.4-r2\n";
        let read = parse_catalog(apk);
        assert_eq!(read.len(), 2);
        assert_eq!(read[0].name, "tcpdump");
        assert_eq!(read[0].version, "4.99.6-r1", "the first of a name is kept");
        assert_eq!(read[0].installed_bytes, Some(890_772));
        assert_eq!(
            read[1].description,
            "Network monitoring and data acquisition tool (minimal version)"
        );
        let read = parse_catalog(opkg);
        assert_eq!(
            read[0].description,
            "Network monitoring and data acquisition tool"
        );
        assert_eq!(read[0].installed_bytes, Some(901_120));
        assert_eq!(
            (read[1].name.as_str(), read[1].installed_bytes),
            ("terminfo", None)
        );
    }

    #[test]
    fn upgrades_name_both_versions() {
        let opkg = "luci-mod-system - 26.205.85413~cac97ed - 26.301.70438~aa4865e\n\
                    ucode-mod-ubus - 2025.07.18~3f64c808-r1 - 2025.07.18~3f64c808-r2\n";
        let read = parse_upgrades(PackageManager::Opkg, opkg, &[]);
        assert_eq!(read.len(), 2);
        assert_eq!(read[1].name, "ucode-mod-ubus");
        assert_eq!(read[1].from, "2025.07.18~3f64c808-r1");
        assert_eq!(read[1].to, "2025.07.18~3f64c808-r2");
        let installed = [Package {
            name: "dnsmasq".into(),
            version: "2.93-r1".into(),
            explicit: true,
        }];
        let apk =
            "Name: dnsmasq\nVersion: 2.93-r2\n\nName: jshn\nVersion: 2026.11.02~ff13b89e-r1\n";
        let read = parse_upgrades(PackageManager::Apk, apk, &installed);
        assert_eq!(
            (read[0].from.as_str(), read[0].to.as_str()),
            ("2.93-r1", "2.93-r2")
        );
        assert_eq!(
            read[1].from, "",
            "a version the inventory lacks stays unknown"
        );
    }

    #[test]
    fn the_rom_s_packages_are_named_by_either_database() {
        assert_eq!(parse_rom("P:busybox\nP:luci\n"), ["busybox", "luci"]);
        assert_eq!(parse_rom("Package: busybox\n"), ["busybox"]);
        assert_eq!(parse_rom(""), Vec::<String>::new());
        assert_eq!(rom(PackageManager::Opkg)[3], "/rom/usr/lib/opkg/status");
        assert_eq!(catalog(PackageManager::Opkg)[0], "sh");
        assert_eq!(upgradable(PackageManager::Apk)[1], "query");
    }
}
