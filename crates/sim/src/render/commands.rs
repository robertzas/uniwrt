//! The commands the jobs run on a node, answered as OpenWrt would: adoption's
//! (features/adoption.md, Path A: `/etc/openwrt_release`, the script that appends the
//! controller's key to dropbear's `authorized_keys`, the package manager's database (opkg's
//! status file, or apk's installed database and world file) and `sysupgrade -b -`), the
//! transaction tool's and the reads of the current configuration ([`crate::txn`], D-069), and
//! the gateway's DNS check (`nslookup`). Anything else exits 127, as `sh` does for a command it
//! cannot find.

use std::fmt::Write as _;

use uniwrt_node::{ExecOutput, ExecRequest, ExitStatus};

use crate::{
    txn::{Now, Tool},
    world::Node,
};

/// What `sysupgrade -b -` writes: a small backup with harmless settings.
const BACKUP: &[u8] = include_bytes!("../../data/backup.tar.gz");

/// What survives a node's reboots.
#[derive(Debug)]
pub(crate) struct Disk {
    /// The lines of dropbear's `authorized_keys`.
    pub(crate) authorized_keys: Vec<String>,
    /// The transaction tool, `/etc/config` and the init scripts.
    pub(crate) tool: Tool,
}

impl Default for Disk {
    fn default() -> Self {
        Self {
            authorized_keys: Vec::new(),
            tool: Tool::stock(),
        }
    }
}

/// The installed packages, `(name, version, installed on purpose)`, the same on every node.
const PACKAGES: [(&str, &str, bool); 22] = [
    ("base-files", "1647-r1", false),
    ("batctl-full", "2025.4-r1", true),
    ("busybox", "1.37.0-r5", false),
    ("ca-bundle", "20250419-r1", false),
    ("dnsmasq", "2.91-r1", false),
    ("dropbear", "2025.88-r1", false),
    ("firewall4", "2025.07.17~0d9b5dd8-r1", false),
    ("fstools", "2025.06.26~1d7f3cd4-r1", false),
    ("hostapd-common", "2025.08.26~ced4f2b0-r1", false),
    ("iw", "6.9-r2", false),
    ("kmod-batman-adv", "6.12.58.2025.4-r1", true),
    ("libc", "1.2.5-r5", false),
    ("logd", "2025.07.12~c5c3ba7e-r1", false),
    ("netifd", "2025.10.12~1ac2d73a-r1", false),
    ("odhcpd-ipv6only", "2025.10.02~b0e6e1ac-r1", false),
    ("procd", "2025.07.29~6b3b2c4f-r1", false),
    ("rpcd", "2025.08.04~71a0a556-r1", false),
    ("ubus", "2025.07.02~b20eb9e0-r1", false),
    ("ucode", "2025.10.06~a1e9c0d8-r1", false),
    ("ucode-mod-nl80211", "2025.10.06~a1e9c0d8-r1", false),
    ("uci", "2025.07.18~e4bd97b7-r1", false),
    ("wpad-mesh-mbedtls", "2025.08.26~ced4f2b0-r1", true),
];

/// Runs `request` on `node`, whose files are on `disk`, at `now_ms` (the world's clock).
pub(crate) fn exec(node: &Node, request: &ExecRequest, disk: &mut Disk, now_ms: i64) -> ExecOutput {
    let now = Now {
        uptime_s: u64::try_from((now_ms - node.booted_ms) / 1000).unwrap_or(0),
        unix_s: now_ms / 1000,
        booted_ms: node.booted_ms,
    };
    if let Some(output) = disk.tool.exec(&request.argv, request.stdin.as_deref(), now) {
        return output;
    }
    let keys = &mut disk.authorized_keys;
    let argv: Vec<&str> = request.argv.iter().map(String::as_str).collect();
    match argv.as_slice() {
        ["cat", "/etc/openwrt_release"] => ok(release(node).into_bytes()),
        ["sh", "-c", script, "sh", key] if script.contains("/etc/dropbear/authorized_keys") => {
            let answer = if keys.iter().any(|line| line == key) {
                "present"
            } else {
                keys.push((*key).to_owned());
                "added"
            };
            ok(format!("{answer}\n").into_bytes())
        }
        [
            "grep",
            "-E",
            "^(Package|Version|Status):",
            "/usr/lib/opkg/status",
        ] if node.release.package_manager == "opkg" => ok(opkg_status().into_bytes()),
        ["grep", "-E", "^[PV]:", "/lib/apk/db/installed"]
            if node.release.package_manager == "apk" =>
        {
            ok(apk_installed().into_bytes())
        }
        ["cat", "/etc/apk/world"] if node.release.package_manager == "apk" => {
            ok(apk_world().into_bytes())
        }
        ["sysupgrade", "-b", "-"] => ok(BACKUP.to_vec()),
        ["nslookup", name, "127.0.0.1"] if node.gateway => ok(format!(
            "Server:\t\t127.0.0.1\nAddress:\t127.0.0.1:53\n\nName:\t{name}\nAddress: 192.0.2.53\n"
        )
        .into_bytes()),
        [command, ..] => failed(127, format!("sh: {command}: not found\n")),
        [] => failed(127, "sh: : not found\n".to_owned()),
    }
}

fn release(node: &Node) -> String {
    let (model, release) = (node.model, node.release);
    format!(
        "DISTRIB_ID='OpenWrt'\nDISTRIB_RELEASE='{version}'\nDISTRIB_REVISION='{revision}'\n\
         DISTRIB_TARGET='{target}'\nDISTRIB_ARCH='{arch}'\n\
         DISTRIB_DESCRIPTION='OpenWrt {version} {revision}'\nDISTRIB_TAINTS=''\n",
        version = release.version,
        revision = release.revision,
        target = model.target,
        arch = model.arch,
    )
}

fn opkg_status() -> String {
    let mut out = String::new();
    for (name, version, wanted) in PACKAGES {
        let flag = if wanted { "user" } else { "ok" };
        let _ = writeln!(
            out,
            "Package: {name}\nVersion: {version}\nStatus: install {flag} installed"
        );
    }
    out
}

fn apk_installed() -> String {
    let mut out = String::new();
    for (name, version, _) in PACKAGES {
        let _ = writeln!(out, "P:{name}\nV:{version}");
    }
    out
}

fn apk_world() -> String {
    let mut out = String::new();
    for (name, _, wanted) in PACKAGES {
        if wanted {
            let _ = writeln!(out, "{name}");
        }
    }
    out
}

fn ok(stdout: Vec<u8>) -> ExecOutput {
    ExecOutput {
        status: ExitStatus::Code(0),
        stdout,
        stderr: Vec::new(),
    }
}

fn failed(code: u32, stderr: String) -> ExecOutput {
    ExecOutput {
        status: ExitStatus::Code(code),
        stdout: Vec::new(),
        stderr: stderr.into_bytes(),
    }
}
