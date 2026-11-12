//! The commands the jobs run on a node, answered as OpenWrt would: adoption's
//! (features/adoption.md, Path A: `/etc/openwrt_release`, the script that appends the
//! controller's key to dropbear's `authorized_keys`, the package manager's database (opkg's
//! status file, or apk's installed database and world file) and `sysupgrade -b -`), the
//! transaction tool's and the reads of the current configuration ([`crate::txn`], D-069), and
//! the gateway's DNS check (`nslookup`). Anything else exits 127, as `sh` does for a command it
//! cannot find.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
};

use uniwrt_node::{ExecOutput, ExecRequest, ExitStatus, proto::ops::NeighborReport};

use crate::{
    hand_edit::HandEdit,
    txn::{Now, Tool},
    world::{Node, World, cables::Wiring},
};

/// Root's password on a node that was set up by hand: the demo's, so not secret.
pub const ROOT_PASSWORD: &str = "demo";

/// What `sysupgrade -b -` writes: a small backup with harmless settings.
const BACKUP: &[u8] = include_bytes!("../../data/backup.tar.gz");

/// What survives a node's reboots.
#[derive(Debug)]
pub(crate) struct Disk {
    /// The lines of dropbear's `authorized_keys`.
    pub(crate) authorized_keys: Vec<String>,
    /// Root's password; empty on a factory-default node, where dropbear lets root in without
    /// one.
    pub(crate) root_password: String,
    /// How many of the tool's bounces the LAN has followed ([`crate::link`]).
    pub(crate) bounces_followed: usize,
    /// The transaction tool, `/etc/config` and the init scripts.
    pub(crate) tool: Tool,
    /// Demo mode's drift, still to come or done.
    pub(crate) hand_edit: HandEdit,
    /// The packages installed from the feeds since the image ([`super::feeds`]).
    pub(crate) installed: BTreeSet<String>,
    /// The wpad installed ([`super::wpad`]): the mesh one, with 802.11v, on a node set up by
    /// hand; the image's basic one on a factory gateway, until the wpad swap.
    pub(crate) wpad: &'static str,
    /// The BSSes the `bss` op turned off, and `/etc/config/wireless` as it was then: a
    /// `wifi reload` (an apply that changes it) brings them back ([`Disk::bss_off`]).
    pub(crate) bss_off: Option<(String, BTreeSet<String>)>,
    /// The neighbor lists the `rrm` op set, by interface, with the `wireless` they were set
    /// under: hostapd forgets them when a reload restarts the BSSes ([`Disk::rrm`]).
    pub(crate) rrm: Option<(String, BTreeMap<String, Vec<NeighborReport>>)>,
    /// `/etc/config/network` as [`Disk::rewired`] last read it.
    pub(crate) wired_from: Option<String>,
}

impl Default for Disk {
    /// A node with stock services and no `/etc/config`.
    fn default() -> Self {
        Self {
            authorized_keys: Vec::new(),
            root_password: ROOT_PASSWORD.to_owned(),
            bounces_followed: 0,
            tool: Tool::stock(),
            hand_edit: HandEdit::default(),
            installed: BTreeSet::new(),
            wpad: super::wpad::MESH,
            bss_off: None,
            rrm: None,
            wired_from: None,
        }
    }
}

impl Disk {
    /// The BSSes that are off: those the `bss` op turned off, unless `wireless` changed since.
    pub(crate) fn bss_off(&self) -> BTreeSet<String> {
        match &self.bss_off {
            Some((wireless, off)) if self.tool.files.get("wireless") == Some(wireless) => {
                off.clone()
            }
            _ => BTreeSet::new(),
        }
    }

    /// Whether usteer steers clients here: it is installed with its config and enabled, and
    /// the wpad has 802.11v (not the basic one), so hostapd can send its requests.
    pub(crate) fn steers(&self) -> bool {
        self.installed.contains("usteer")
            && self.tool.files.contains_key("usteer")
            && self.tool.services.get("usteer") == Some(&true)
            && self.wpad != super::wpad::BASIC
    }

    /// The neighbor lists hostapd holds: those set since `wireless` last changed.
    pub(crate) fn rrm(&self) -> BTreeMap<String, Vec<NeighborReport>> {
        match &self.rrm {
            Some((wireless, lists)) if self.tool.files.get("wireless") == Some(wireless) => {
                lists.clone()
            }
            _ => BTreeMap::new(),
        }
    }

    /// What `network` does with the ports, once it changed since the last call.
    pub(crate) fn rewired(&mut self) -> Option<Wiring> {
        let network = self.tool.files.get("network");
        if self.wired_from.as_ref() == network && self.wired_from.is_some() {
            return None;
        }
        self.wired_from = Some(network.cloned().unwrap_or_default());
        Some(Wiring::of(network.map(String::as_str)))
    }

    /// Node `n`'s, configured by hand to run the simulated network ([`super::config`]); a
    /// `factory` gateway's is OpenWrt's own, and its root has no password.
    pub(crate) fn of(world: &World, n: usize, factory: bool) -> Self {
        let node = &world.nodes[n];
        let factory = factory && node.gateway;
        let mut tool = Tool::stock();
        tool.files = if factory {
            super::config::factory_files(node)
        } else {
            super::config::files(world, n)
        };
        if !node.gateway {
            for service in super::config::AP_OFF {
                tool.services.insert(service.to_owned(), false);
            }
        }
        Self {
            root_password: if factory {
                String::new()
            } else {
                ROOT_PASSWORD.to_owned()
            },
            tool,
            wpad: if factory {
                super::wpad::BASIC
            } else {
                super::wpad::MESH
            },
            ..Self::default()
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

/// `node`'s clock at `now_ms` (the world's).
pub(crate) fn now(node: &Node, now_ms: i64) -> Now {
    Now {
        uptime_s: u64::try_from((now_ms - node.booted_ms) / 1000).unwrap_or(0),
        unix_s: now_ms / 1000,
        booted_ms: node.booted_ms,
    }
}

/// Runs `request` on `node`, whose files are on `disk`, at `now_ms` (the world's clock).
pub(crate) fn exec(node: &Node, request: &ExecRequest, disk: &mut Disk, now_ms: i64) -> ExecOutput {
    let now = now(node, now_ms);
    if let Some(output) = disk.tool.exec(&request.argv, request.stdin.as_deref(), now) {
        return output;
    }
    let argv: Vec<&str> = request.argv.iter().map(String::as_str).collect();
    if let Some(output) = super::wpad::exec(&argv, node.release.package_manager, &mut disk.wpad) {
        return output;
    }
    if let Some(output) = super::feeds::exec(
        &argv,
        node.release.package_manager,
        &mut disk.installed,
        &mut disk.tool,
    ) {
        return output;
    }
    if node.gateway
        && let Some(output) = wan_check(node, &argv, &disk.tool.files)
    {
        return output;
    }
    if node.gateway
        && let ["sh", "-s", "--", phase @ ..] = argv.as_slice()
    {
        let files = &disk.tool.files;
        let up = crate::isp::uplink(
            files.get("network").map(String::as_str),
            node.model.wan_port,
        )
        .ipv4();
        let running = disk.tool.services.get("sqm") == Some(&true);
        let shaped = crate::line::shaped(files.get("sqm").map(String::as_str), running);
        return ok(crate::line::speed_test(phase, up, shaped, now.uptime_s * 100).into_bytes());
    }
    if let ["passwd", "root"] = argv.as_slice() {
        return passwd(request.stdin.as_deref(), &mut disk.root_password);
    }
    let keys = &mut disk.authorized_keys;
    match argv.as_slice() {
        ["cat", "/etc/openwrt_release"] => ok(release(node).into_bytes()),
        ["ubus", "call", "system", "board"] => {
            ok(super::system::board(node)["board"].to_string().into_bytes())
        }
        // The controller logs in from host .50 of the LAN.
        ["env"] => {
            let [a, b, c, _] = node.ip.octets();
            ok(format!(
                "USER=root\nSSH_CONNECTION={a}.{b}.{c}.50 50022 {} 22\nHOME=/root\n",
                node.ip
            )
            .into_bytes())
        }
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
        ] if node.release.package_manager == "opkg" => {
            ok(opkg_status(&disk.installed, disk.wpad).into_bytes())
        }
        ["grep", "-E", "^[PV]:", "/lib/apk/db/installed"]
            if node.release.package_manager == "apk" =>
        {
            ok(apk_installed(&disk.installed, disk.wpad).into_bytes())
        }
        ["cat", "/etc/apk/world"] if node.release.package_manager == "apk" => {
            ok(apk_world(&disk.installed, disk.wpad).into_bytes())
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

/// The WAN test's commands on the gateway (`ping`, `nslookup` through a resolver, `wget`): they
/// answer once the ISP gave the WAN what they need.
fn wan_check(
    node: &Node,
    argv: &[&str],
    files: &std::collections::BTreeMap<String, String>,
) -> Option<ExecOutput> {
    let uplink = crate::isp::uplink(
        files.get("network").map(String::as_str),
        node.model.wan_port,
    );
    let ipv6 = uplink.wan6.as_ref().is_some_and(|(_, up)| *up);
    let reached = match argv {
        ["ping", "-6", ..] => ipv6,
        ["ping" | "wget", ..] => uplink.ipv4(),
        ["nslookup", _, server] if *server != "127.0.0.1" => uplink.ipv4(),
        _ => return None,
    };
    Some(match (argv[0], reached) {
        ("ping", true) => ok(
            b"3 packets transmitted, 3 packets received, 0% packet loss\n\
                              round-trip min/avg/max = 3.912/4.410/5.208 ms\n"
                .to_vec(),
        ),
        ("ping", false) => failed(1, "ping: sendto: Network unreachable\n".to_owned()),
        ("nslookup", true) => ok(format!(
            "Server:\t\t{server}\nAddress:\t{server}:53\n\nName:\t{name}\nAddress: 192.0.2.53\n",
            server = argv[2],
            name = argv[1],
        )
        .into_bytes()),
        ("nslookup", false) => failed(
            1,
            ";; connection timed out; no servers could be reached\n".to_owned(),
        ),
        (_, true) => ok(Vec::new()),
        (_, false) => failed(4, "wget: download timed out\n".to_owned()),
    })
}

/// BusyBox's `passwd`: the new password twice on stdin.
fn passwd(stdin: Option<&[u8]>, password: &mut String) -> ExecOutput {
    let typed = String::from_utf8_lossy(stdin.unwrap_or_default());
    let mut lines = typed.lines();
    match (lines.next(), lines.next()) {
        (Some(new), Some(again)) if new == again && !new.is_empty() => {
            new.clone_into(password);
            ok(b"Changing password for root
New password: 
Retype password: 
                 passwd: password for root changed by root
"
            .to_vec())
        }
        _ => failed(
            1,
            "passwd: passwords don't match
"
            .to_owned(),
        ),
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

fn opkg_status(installed: &BTreeSet<String>, wpad: &'static str) -> String {
    let mut out = String::new();
    for (name, version, wanted) in all(installed, wpad) {
        let flag = if wanted { "user" } else { "ok" };
        let _ = writeln!(
            out,
            "Package: {name}\nVersion: {version}\nStatus: install {flag} installed"
        );
    }
    out
}

/// The image's packages (with the node's wpad) and those installed since, `(name, version,
/// installed on purpose)`.
fn all<'a>(
    installed: &'a BTreeSet<String>,
    wpad: &'static str,
) -> impl Iterator<Item = (&'a str, &'a str, bool)> {
    PACKAGES
        .into_iter()
        .map(move |(name, version, wanted)| {
            if name == super::wpad::MESH {
                (wpad, version, wanted)
            } else {
                (name, version, wanted)
            }
        })
        .chain(installed.iter().filter_map(|name| {
            super::feeds::version(name).map(|version| (name.as_str(), version, true))
        }))
}

fn apk_installed(installed: &BTreeSet<String>, wpad: &'static str) -> String {
    let mut out = String::new();
    for (name, version, _) in all(installed, wpad) {
        let _ = writeln!(out, "P:{name}\nV:{version}");
    }
    out
}

fn apk_world(installed: &BTreeSet<String>, wpad: &'static str) -> String {
    let mut out = String::new();
    for (name, _, wanted) in all(installed, wpad) {
        if wanted {
            let _ = writeln!(out, "{name}");
        }
    }
    out
}

pub(crate) fn ok(stdout: Vec<u8>) -> ExecOutput {
    ExecOutput {
        status: ExitStatus::Code(0),
        stdout,
        stderr: Vec::new(),
    }
}

pub(crate) fn failed(code: u32, stderr: String) -> ExecOutput {
    ExecOutput {
        status: ExitStatus::Code(code),
        stdout: Vec::new(),
        stderr: stderr.into_bytes(),
    }
}
