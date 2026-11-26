//! dropbear as the sim's nodes run it (D-150): one instance per enabled `dropbear` section of the
//! node's `dropbear` file, with the flags OpenWrt's init script gives it (`-s` and `-g` for
//! passwords off, `-l` and the interface's device for `DirectInterface`); OpenWrt's one
//! instance, open to passwords, without the file. The demo's logins follow it: a node whose
//! servers all refuse passwords takes keys alone.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use uniwrt_uci::{Package, Section};

fn off(section: &Section, option: &str) -> bool {
    matches!(
        section.option(option),
        Some("0" | "off" | "no" | "false" | "disabled")
    )
}

fn sections(files: &BTreeMap<String, String>) -> Option<Vec<Section>> {
    let package = uniwrt_uci::parse("dropbear", files.get("dropbear")?).ok()?;
    let enabled = package
        .sections_of("dropbear")
        .filter(|s| !off(s, "enable"))
        .cloned()
        .collect();
    Some(enabled)
}

/// The `ssh` field of the `services` set: each instance's `running` and command line.
pub(super) fn instances(files: &BTreeMap<String, String>, network: Option<&Package>) -> Value {
    let Some(sections) = sections(files) else {
        return json!([{ "running": true, "command": ["/usr/sbin/dropbear", "-F", "-p", "22"] }]);
    };
    let list: Vec<Value> = sections
        .iter()
        .map(|s| {
            let mut command = vec!["/usr/sbin/dropbear".to_owned(), "-F".to_owned()];
            let device = s
                .option("DirectInterface")
                .and_then(|iface| network?.section(iface)?.option("device"));
            if let Some(device) = device {
                command.extend(["-l".to_owned(), device.to_owned()]);
            }
            command.extend(["-p".to_owned(), s.option("Port").unwrap_or("22").to_owned()]);
            if off(s, "PasswordAuth") {
                command.push("-s".to_owned());
            }
            if off(s, "RootPasswordAuth") {
                command.push("-g".to_owned());
            }
            json!({ "running": true, "command": command })
        })
        .collect();
    Value::Array(list)
}

/// [`instances`] for a node whose `/etc/config` is `files`: what its `services` set reports.
pub fn servers(files: &BTreeMap<String, String>) -> Value {
    let network = files
        .get("network")
        .and_then(|text| uniwrt_uci::parse("network", text).ok());
    instances(files, network.as_ref())
}

/// Whether root logs in with a password: some server takes passwords, and root's.
pub(crate) fn passwords(files: &BTreeMap<String, String>) -> bool {
    sections(files).is_none_or(|sections| {
        sections
            .iter()
            .any(|s| !off(s, "PasswordAuth") && !off(s, "RootPasswordAuth"))
    })
}
