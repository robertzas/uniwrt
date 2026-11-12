//! The package feeds as an apply's install step meets them (D-076): `opkg update` / `apk
//! update`, then installing what SQM, failover, UPnP and steering need, which brings their init
//! scripts (enabled, as postinst scripts do) and their stock `/etc/config` files; and the init
//! scripts' `start`, `stop`, `enable` and `disable`.

use std::collections::BTreeSet;

use uniwrt_node::ExecOutput;

use super::commands::{failed, ok};
use crate::txn::Tool;

/// A package of the feeds: name, version, the init script it brings, and its stock
/// `/etc/config` file (package, text).
type FeedPackage = (
    &'static str,
    &'static str,
    Option<&'static str>,
    Option<(&'static str, &'static str)>,
);

/// The packages the feeds have beyond the image's.
pub(crate) const FEED: [FeedPackage; 7] = [
    (
        "sqm-scripts",
        "1.6.0-r1",
        Some("sqm"),
        Some((
            "sqm",
            "config queue 'eth1'\n\toption enabled '0'\n\toption interface 'eth1'\n\
             \toption download '85000'\n\toption upload '10000'\n\toption qdisc 'cake'\n\
             \toption script 'piece_of_cake.qos'\n\toption linklayer 'none'\n",
        )),
    ),
    ("kmod-sched-cake", "6.12.58-r1", None, None),
    (
        "mwan3",
        "2.11.19-r1",
        Some("mwan3"),
        Some((
            "mwan3",
            "config globals 'globals'\n\toption mmx_mask '0x3F00'\n",
        )),
    ),
    (
        "miniupnpd-nftables",
        "2.3.9-r1",
        Some("miniupnpd"),
        Some((
            "upnpd",
            "config upnpd 'config'\n\toption enabled '0'\n\toption enable_natpmp '1'\n\
             \toption enable_upnp '1'\n\toption secure_mode '1'\n\toption log_output '0'\n\
             \toption download '1024'\n\toption upload '512'\n\toption internal_iface 'lan'\n\
             \toption port '5000'\n\toption upnp_lease_file '/var/run/miniupnpd.leases'\n\
             \toption igdv1 '1'\n\nconfig perm_rule\n\toption action 'allow'\n\
             \toption ext_ports '1024-65535'\n\toption int_addr '0.0.0.0/0'\n\
             \toption int_ports '1024-65535'\n\toption comment 'Allow high ports'\n\n\
             config perm_rule\n\toption action 'deny'\n\toption ext_ports '0-65535'\n\
             \toption int_addr '0.0.0.0/0'\n\toption int_ports '0-65535'\n\
             \toption comment 'Default deny'\n",
        )),
    ),
    (
        "usteer",
        "2025.10.04~1d6524c6-r1",
        Some("usteer"),
        Some((
            "usteer",
            "config usteer\n\toption 'network' 'lan'\n\toption 'syslog' '1'\n\
             \toption local_mode '0'\n\toption 'ipv6' '0'\n\toption 'debug_level' '2'\n",
        )),
    ),
    ("kmod-usb-net-rndis", "6.12.58-r1", None, None),
    ("kmod-usb-net-cdc-ether", "6.12.58-r1", None, None),
];

/// The version of a package from the feeds.
pub(crate) fn version(name: &str) -> Option<&'static str> {
    FEED.iter()
        .find(|(feed, ..)| *feed == name)
        .map(|(_, version, ..)| *version)
}

/// Answers the package manager's and the init scripts' commands; `None` for others.
pub(crate) fn exec(
    argv: &[&str],
    manager: &str,
    installed: &mut BTreeSet<String>,
    tool: &mut Tool,
) -> Option<ExecOutput> {
    let add = if manager == "apk" { "add" } else { "install" };
    Some(match argv {
        [tool_name, "update"] if *tool_name == manager => ok(Vec::new()),
        [tool_name, verb, packages @ ..] if *tool_name == manager && *verb == add => {
            if let Some(unknown) = packages.iter().find(|p| version(p).is_none()) {
                return Some(if manager == "apk" {
                    failed(
                        1,
                        format!(
                            "ERROR: unable to select packages:\n  {unknown} (no such package)\n"
                        ),
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
            for package in packages {
                installed.insert((*package).to_owned());
                let Some((_, _, service, config)) = FEED.iter().find(|(name, ..)| name == package)
                else {
                    continue;
                };
                if let Some(service) = service {
                    tool.services.insert((*service).to_owned(), true);
                }
                if let Some((name, text)) = config {
                    tool.files
                        .entry((*name).to_owned())
                        .or_insert_with(|| (*text).to_owned());
                }
            }
            ok(Vec::new())
        }
        [script, action] if script.starts_with("/etc/init.d/") => {
            let name = &script["/etc/init.d/".len()..];
            let Some(enabled) = tool.services.get_mut(name) else {
                return Some(failed(127, format!("sh: {script}: not found\n")));
            };
            match *action {
                "enable" => *enabled = true,
                "disable" => *enabled = false,
                "start" | "stop" | "restart" | "reload" => {}
                _ => return None,
            }
            ok(Vec::new())
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::exec;
    use crate::txn::Tool;

    #[test]
    fn installing_brings_the_service_and_its_config() {
        let mut tool = Tool::stock();
        let mut installed = BTreeSet::new();
        let ok = |out: Option<uniwrt_node::ExecOutput>| out.unwrap().status.success();
        assert!(ok(exec(
            &["apk", "update"],
            "apk",
            &mut installed,
            &mut tool
        )));
        assert!(ok(exec(
            &["apk", "add", "sqm-scripts", "kmod-sched-cake"],
            "apk",
            &mut installed,
            &mut tool
        )));
        assert!(installed.contains("sqm-scripts"));
        assert_eq!(tool.services.get("sqm"), Some(&true));
        assert!(tool.files["sqm"].contains("option enabled '0'"));
        assert!(ok(exec(
            &["/etc/init.d/sqm", "disable"],
            "apk",
            &mut installed,
            &mut tool
        )));
        assert_eq!(tool.services.get("sqm"), Some(&false));
        assert!(ok(exec(
            &["apk", "add", "usteer"],
            "apk",
            &mut installed,
            &mut tool
        )));
        let mut disk = super::super::commands::Disk {
            installed: installed.clone(),
            tool: tool.clone(),
            ..super::super::commands::Disk::default()
        };
        assert!(
            disk.steers(),
            "usteer runs where it is installed and enabled"
        );
        disk.wpad = super::super::wpad::BASIC;
        assert!(!disk.steers(), "the basic wpad has no 802.11v");
        disk.wpad = super::super::wpad::MESH;
        disk.tool.services.insert("usteer".to_owned(), false);
        assert!(!disk.steers(), "a disabled usteer steers nothing");
        let unknown = exec(&["apk", "add", "nope"], "apk", &mut installed, &mut tool).unwrap();
        assert!(!unknown.status.success());
        assert!(
            exec(
                &["opkg", "install", "mwan3"],
                "apk",
                &mut installed,
                &mut tool
            )
            .is_none()
        );
    }
}
