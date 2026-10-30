//! The installed packages, read from the package manager's database without running it: opkg's
//! status file, or apk's installed database plus its world file (the packages wanted on
//! purpose). Only the fields needed are sent, through `grep`.

use std::collections::BTreeSet;

use uniwrt_model::{facts::PackageManager, inventory::Package};

/// The commands whose outputs [`parse`] reads, in order.
pub(super) fn commands(manager: PackageManager) -> Vec<Vec<&'static str>> {
    match manager {
        PackageManager::Opkg => vec![vec![
            "grep",
            "-E",
            "^(Package|Version|Status):",
            "/usr/lib/opkg/status",
        ]],
        PackageManager::Apk => vec![
            vec!["grep", "-E", "^[PV]:", "/lib/apk/db/installed"],
            vec!["cat", "/etc/apk/world"],
        ],
    }
}

/// The packages in the outputs of [`commands`], sorted by name.
pub(super) fn parse(manager: PackageManager, outputs: &[String]) -> Vec<Package> {
    let mut packages = match manager {
        PackageManager::Opkg => opkg(outputs.first().map_or("", String::as_str)),
        PackageManager::Apk => apk(
            outputs.first().map_or("", String::as_str),
            outputs.get(1).map_or("", String::as_str),
        ),
    };
    packages.sort_by(|a, b| a.name.cmp(&b.name));
    packages
}

/// opkg's status: a `Package:` line starts each package; `Status: install user installed` marks
/// one installed on purpose, `install ok installed` a dependency.
fn opkg(status: &str) -> Vec<Package> {
    let mut packages = Vec::new();
    let mut current: Option<(Package, bool)> = None;
    let mut finish = |current: Option<(Package, bool)>| {
        if let Some((package, true)) = current {
            packages.push(package);
        }
    };
    for line in status.lines() {
        let Some((field, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match field {
            "Package" => {
                finish(current.take());
                current = Some((
                    Package {
                        name: value.to_owned(),
                        version: String::new(),
                        explicit: false,
                    },
                    false,
                ));
            }
            "Version" => {
                if let Some((package, _)) = &mut current {
                    value.clone_into(&mut package.version);
                }
            }
            "Status" => {
                if let Some((package, installed)) = &mut current {
                    let words: Vec<_> = value.split_whitespace().collect();
                    *installed = words.last() == Some(&"installed");
                    package.explicit = words.contains(&"user");
                }
            }
            _ => {}
        }
    }
    finish(current);
    packages
}

/// apk's installed database (`P:` name, `V:` version per package) and its world file: one
/// dependency per line, a name optionally followed by a version constraint.
fn apk(installed: &str, world: &str) -> Vec<Package> {
    let wanted: BTreeSet<&str> = world
        .split_whitespace()
        .filter(|entry| !entry.starts_with('!'))
        .map(|entry| entry.split(['=', '<', '>', '~']).next().unwrap_or(entry))
        .collect();
    let mut packages: Vec<Package> = Vec::new();
    for line in installed.lines() {
        if let Some(name) = line.strip_prefix("P:") {
            packages.push(Package {
                name: name.to_owned(),
                version: String::new(),
                explicit: wanted.contains(name),
            });
        } else if let (Some(version), Some(package)) =
            (line.strip_prefix("V:"), packages.last_mut())
        {
            version.clone_into(&mut package.version);
        }
    }
    packages
}

#[cfg(test)]
mod tests {
    use uniwrt_model::{facts::PackageManager, inventory::Package};

    use super::parse;

    fn package(name: &str, version: &str, explicit: bool) -> Package {
        Package {
            name: name.to_owned(),
            version: version.to_owned(),
            explicit,
        }
    }

    #[test]
    fn opkg_status_marks_user_installed_packages_and_skips_removed_ones() {
        let status = "\
Package: dropbear
Version: 2024.86-r3
Status: install ok installed
Package: luci
Version: 25.012.12345~abcdef
Status: install user installed
Package: gone
Version: 1.0
Status: deinstall ok not-installed
Package: base-files
Version: 1674~443ec4032a
Status: install ok installed
";
        assert_eq!(
            parse(PackageManager::Opkg, &[status.to_owned()]),
            [
                package("base-files", "1674~443ec4032a", false),
                package("dropbear", "2024.86-r3", false),
                package("luci", "25.012.12345~abcdef", true),
            ]
        );
    }

    #[test]
    fn apk_marks_the_packages_in_world_whatever_their_constraint() {
        let installed =
            "P:dropbear\nV:2025.89-r3\nP:base-files\nV:1718~981c978eff\nP:libc\nV:1.2.5-r5\n";
        let world = "base-files=1718~981c978eff\ndropbear\n!ppp\n";
        assert_eq!(
            parse(
                PackageManager::Apk,
                &[installed.to_owned(), world.to_owned()]
            ),
            [
                package("base-files", "1718~981c978eff", true),
                package("dropbear", "2025.89-r3", true),
                package("libc", "1.2.5-r5", false),
            ]
        );
    }
}
