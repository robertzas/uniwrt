//! The package manager UI's commands as a simulated node answers them (D-139): what its feeds
//! offer beyond [`super::feeds`] ([`EXTRA`]), the newer versions of image packages
//! ([`NEWER`]), `/rom`'s packages (the image's), dry runs, removals (with the dependencies
//! only they needed) and upgrades, for apk and opkg alike.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use uniwrt_node::ExecOutput;

use super::commands::{Disk, failed, image_packages, ok};

/// Packages of the feeds the image lacks, for the UI to offer: name, version, installed size,
/// description.
pub(crate) const EXTRA: [(&str, &str, u64, &str); 5] = [
    (
        "tcpdump",
        "4.99.6-r1",
        890_772,
        "Network monitoring and data acquisition tool",
    ),
    (
        "libpcap1",
        "1.10.6-r1",
        301_204,
        "Low-level packet capture library",
    ),
    ("htop", "3.4.1-r1", 214_302, "Interactive process viewer"),
    (
        "iperf3",
        "3.19.1-r1",
        152_876,
        "Internet Protocol bandwidth measuring tool",
    ),
    (
        "nano",
        "8.6-r1",
        233_015,
        "An enhanced clone of the Pico text editor",
    ),
];
/// `(package, what it depends on)` among [`EXTRA`].
const DEPENDS: [(&str, &str); 1] = [("tcpdump", "libpcap1")];
/// Image packages with a newer version in the feeds.
pub(crate) const NEWER: [(&str, &str); 3] = [
    ("ca-bundle", "20250419-r2"),
    ("dnsmasq", "2.91-r2"),
    ("rpcd", "2025.08.04~71a0a556-r2"),
];
/// What a package of the feeds takes when the index doesn't say otherwise.
const SIZE: u64 = 102_400;

/// The version of an extra package.
pub(crate) fn version(name: &str) -> Option<&'static str> {
    EXTRA
        .iter()
        .find(|(extra, ..)| *extra == name)
        .map(|(_, version, ..)| *version)
}

/// Whether `name` was installed only as another's dependency.
pub(crate) fn dependency(name: &str) -> bool {
    DEPENDS.iter().any(|(_, dep)| *dep == name)
}

/// What the feeds offer: the image's packages (at their newest), the feeds' and the extras.
fn offered() -> Vec<(String, String, u64, String)> {
    let mut all: BTreeMap<String, (String, u64, String)> = BTreeMap::new();
    for (name, version, _) in image_packages() {
        let newer = NEWER
            .iter()
            .find(|(n, _)| *n == name)
            .map_or(version, |(_, v)| *v);
        all.insert(
            name.to_owned(),
            (newer.to_owned(), SIZE, format!("{name} of the image")),
        );
    }
    for (name, version, ..) in super::feeds::FEED {
        all.insert(
            name.to_owned(),
            (version.to_owned(), SIZE, format!("{name} from the feeds")),
        );
    }
    for (name, version, size, description) in EXTRA {
        all.insert(
            name.to_owned(),
            (version.to_owned(), size, description.to_owned()),
        );
    }
    all.into_iter()
        .map(|(name, (version, size, description))| (name, version, size, description))
        .collect()
}

fn installed(disk: &Disk) -> BTreeMap<String, String> {
    super::commands::all(disk)
        .map(|(name, version, _)| (name.to_owned(), version.to_owned()))
        .collect()
}

/// The upgrades on offer: `(name, from, to)`.
fn upgrades(disk: &Disk) -> Vec<(String, String, &'static str)> {
    let now = installed(disk);
    NEWER
        .iter()
        .filter_map(|(name, to)| {
            let from = now.get(*name)?;
            (from != to).then(|| ((*name).to_owned(), from.clone(), *to))
        })
        .collect()
}

/// Who among the installed needs `name`.
fn needed_by(name: &str, now: &BTreeMap<String, String>) -> Vec<String> {
    DEPENDS
        .iter()
        .filter(|(pkg, dep)| *dep == name && now.contains_key(*pkg))
        .map(|(pkg, _)| (*pkg).to_owned())
        .collect()
}

/// What removing `names` takes: them, then the dependencies only they needed.
fn removal(names: &[&str], now: &BTreeMap<String, String>) -> Vec<String> {
    let mut gone: Vec<String> = names.iter().map(|n| (*n).to_owned()).collect();
    for (pkg, dep) in DEPENDS {
        if names.contains(&pkg) && now.contains_key(dep) && !names.contains(&dep) {
            gone.push(dep.to_owned());
        }
    }
    gone
}

/// Answers the package manager UI's commands; `None` for others.
pub(crate) fn exec(argv: &[&str], manager: &str, disk: &mut Disk) -> Option<ExecOutput> {
    let apk = manager == "apk";
    Some(match argv {
        [
            "apk",
            "query",
            "--fields",
            "name,version,installed-size,description",
            "--available",
            "*",
        ] if apk => {
            let mut out = String::new();
            for (name, version, size, description) in offered() {
                let _ = write!(
                    out,
                    "Name: {name}\nVersion: {version}\nDescription: {description}\nInstalled-Size: {size}\n\n"
                );
            }
            ok(out.into_bytes())
        }
        ["sh", "-c", script] if !apk && script.contains("/var/opkg-lists/") => {
            let mut out = String::new();
            for (name, version, size, description) in offered() {
                let _ = write!(
                    out,
                    "Package: {name}\nVersion: {version}\nInstalled-Size: {size}\nDescription:  {description}\n"
                );
            }
            ok(out.into_bytes())
        }
        [
            "apk",
            "query",
            "--fields",
            "name,version",
            "--upgradable",
            "*",
        ] if apk => {
            let mut out = String::new();
            for (name, _, to) in upgrades(disk) {
                let _ = write!(out, "Name: {name}\nVersion: {to}\n\n");
            }
            ok(out.into_bytes())
        }
        ["opkg", "list-upgradable"] if !apk => {
            let mut out = String::new();
            for (name, from, to) in upgrades(disk) {
                let _ = writeln!(out, "{name} - {from} - {to}");
            }
            ok(out.into_bytes())
        }
        ["grep", "-E", field, path] if path.starts_with("/rom/") => {
            let prefix = if *field == "^P:" { "P:" } else { "Package: " };
            let mut out = String::new();
            for (name, ..) in image_packages() {
                let _ = writeln!(out, "{prefix}{name}");
            }
            ok(out.into_bytes())
        }
        ["apk", verb, "-s", names @ ..] if apk => dry(verb, names, true, disk),
        ["opkg", verb, "--noaction", rest @ ..] if !apk => {
            let names = rest.strip_prefix(&["--autoremove"]).unwrap_or(rest);
            dry(verb, names, false, disk)
        }
        ["apk", "del", names @ ..] if apk => remove(names, true, disk),
        ["opkg", "remove", rest @ ..] if !apk => {
            let names = rest.strip_prefix(&["--autoremove"]).unwrap_or(rest);
            remove(names, false, disk)
        }
        [tool, "upgrade", names @ ..] if *tool == manager => {
            let offered = upgrades(disk);
            for name in names {
                let image = image_packages().find(|(image, ..)| image == name);
                if let (Some((image, ..)), Some((_, _, to))) =
                    (image, offered.iter().find(|(n, ..)| n == name))
                {
                    disk.upgraded.insert(image, to);
                }
            }
            ok(Vec::new())
        }
        [tool, verb, names @ ..]
            if *tool == manager
                && (*verb == "add" || *verb == "install")
                && !names.is_empty()
                && names.iter().all(|name| version(name).is_some()) =>
        {
            for name in names {
                disk.installed.insert((*name).to_owned());
                for (pkg, dep) in DEPENDS {
                    if pkg == *name {
                        disk.installed.insert(dep.to_owned());
                    }
                }
            }
            ok(Vec::new())
        }
        _ => return None,
    })
}

/// A dry run of `verb` on `names`, in the manager's words.
fn dry(verb: &str, names: &[&str], apk: bool, disk: &Disk) -> ExecOutput {
    let lines = match verb {
        "add" | "install" => dry_add(names, apk, disk),
        "upgrade" => Ok(dry_upgrade(names, apk, disk)),
        "del" | "remove" => dry_del(names, apk, disk),
        _ => Err(failed(1, format!("unknown command {verb}\n"))),
    };
    let lines = match lines {
        Ok(lines) => lines,
        Err(output) => return output,
    };
    let mut out = String::new();
    let count = lines.len();
    for (i, line) in lines.into_iter().enumerate() {
        if apk {
            let _ = writeln!(out, "({}/{count}) {line}", i + 1);
        } else {
            let _ = writeln!(out, "{line}");
        }
    }
    if apk {
        out.push_str("OK: 11.3 MiB in 136 packages\n");
    }
    ok(out.into_bytes())
}

fn dry_add(names: &[&str], apk: bool, disk: &Disk) -> Result<Vec<String>, ExecOutput> {
    let now = installed(disk);
    let mut lines = Vec::new();
    if let Some(unknown) = names
        .iter()
        .find(|name| version(name).is_none() && super::feeds::version(name).is_none())
    {
        return Err(if apk {
            failed(
                1,
                format!("ERROR: unable to select packages:\n  {unknown} (no such package)\n"),
            )
        } else {
            failed(
                255,
                format!(
                    "Collected errors:\n * opkg_install_cmd: Cannot install package {unknown}.\n"
                ),
            )
        });
    }
    let mut adding: Vec<(String, &str)> = Vec::new();
    for name in names {
        for (pkg, dep) in DEPENDS {
            if pkg == *name && !now.contains_key(dep) {
                adding.push((dep.to_owned(), version(dep).unwrap_or("1")));
            }
        }
        let v = version(name)
            .or_else(|| super::feeds::version(name))
            .unwrap_or("1");
        adding.push(((*name).to_owned(), v));
    }
    for (name, v) in adding {
        lines.push(if apk {
            format!("Installing {name} ({v})")
        } else {
            format!("Installing {name} ({v}) to root...")
        });
    }
    Ok(lines)
}

fn dry_upgrade(names: &[&str], apk: bool, disk: &Disk) -> Vec<String> {
    let mut lines = Vec::new();
    let offered = upgrades(disk);
    for name in names {
        if let Some((_, from, to)) = offered.iter().find(|(n, ..)| n == name) {
            lines.push(if apk {
                format!("Upgrading {name} ({from} -> {to})")
            } else {
                format!("Upgrading {name} on root from {from} to {to}...")
            });
        }
    }
    lines
}

fn dry_del(names: &[&str], apk: bool, disk: &Disk) -> Result<Vec<String>, ExecOutput> {
    let now = installed(disk);
    let mut lines = Vec::new();
    if let Some(missing) = names.iter().find(|name| !now.contains_key(**name)) {
        return Err(if apk {
            failed(1, format!("ERROR: No such package: {missing}\n"))
        } else {
            ok(b"No packages removed.\n".to_vec())
        });
    }
    for name in names {
        let needed = needed_by(name, &now);
        if needed.iter().any(|by| !names.contains(&by.as_str())) {
            return Err(if apk {
                ok(format!("World updated, but the following packages are not removed due to:\n  {name}: {}\n\nOK\n", needed.join(" ")).into_bytes())
            } else {
                let mut said = format!(
                    "No packages removed.\nCollected errors:\n * print_dependents_warning: Package {name} is depended upon by packages:\n"
                );
                for by in &needed {
                    let _ = writeln!(said, " * print_dependents_warning: \t{by}");
                }
                ExecOutput {
                    status: uniwrt_node::ExitStatus::Code(255),
                    stdout: said.into_bytes(),
                    stderr: Vec::new(),
                }
            });
        }
    }
    for name in removal(names, &now) {
        let v = now.get(&name).cloned().unwrap_or_default();
        lines.push(if apk {
            format!("Purging {name} ({v})")
        } else {
            format!("Removing package {name} from root...")
        });
    }
    Ok(lines)
}

fn remove(names: &[&str], apk: bool, disk: &mut Disk) -> ExecOutput {
    let now = installed(disk);
    let refused = dry("del", names, apk, disk);
    let said = String::from_utf8_lossy(&refused.stdout);
    if !refused.status.success()
        || said.contains("not removed")
        || said.contains("No packages removed")
    {
        return refused;
    }
    let image: BTreeSet<&str> = image_packages().map(|(name, ..)| name).collect();
    for name in removal(names, &now) {
        if !disk.installed.remove(&name) && image.contains(name.as_str()) {
            disk.removed.insert(name);
        }
    }
    ok(Vec::new())
}
