//! The per-node package manager (features/network.md, Other services; D-139): a node's installed
//! packages with what keeps each one in place ([`NodePackages`], [`PackageLock`]), what its
//! feeds offer ([`PackageCatalog`]) and the changes an admin asks for ([`PackageChanges`]).
//! [`essential`] is the data of the packages never removed, [`with_firmware`] of those never
//! upgraded on their own, and [`valid_name`] the names a package manager may be given.

use serde::{Deserialize, Serialize};
use uniwrt_core::Timestamp;

/// Why an installed package can't be removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum PackageLock {
    /// The node's configuration needs it (render's requirements, D-076): SQM's, usteer, DoH…
    Required,
    /// The system, uniwrt's own access or the node's role depends on it ([`essential`]).
    Essential,
}

/// An installed package as the package manager UI shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct InstalledPackage {
    pub name: String,
    pub version: String,
    /// Installed on purpose (in the image's or the user's list), not only as a dependency.
    pub explicit: bool,
    /// Why it can't be removed, if it can't.
    pub lock: Option<PackageLock>,
    /// It comes with the firmware: upgraded only by a firmware upgrade ([`with_firmware`]).
    pub with_firmware: bool,
}

/// A node's installed packages, from its inventory (read at adoption and after each change).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct NodePackages {
    /// When the packages were read; `None` when the controller has no inventory of the node.
    pub taken_at: Option<Timestamp>,
    /// By name.
    pub packages: Vec<InstalledPackage>,
    /// Why the packages the site requires on the node can't be told (its configuration doesn't
    /// render): until it does, none is marked `required` and nothing can be removed.
    pub required_unknown: Option<String>,
}

/// A package the node's feeds offer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct AvailablePackage {
    pub name: String,
    pub version: String,
    /// What it takes on flash once installed, when the index says.
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub installed_bytes: Option<u64>,
    pub description: String,
}

/// An installed package with a newer version in the feeds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PackageUpgrade {
    pub name: String,
    pub from: String,
    pub to: String,
}

/// What a node's feeds offer, read from the node with its index refreshed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PackageCatalog {
    pub read_at: Timestamp,
    /// By name, one version each.
    pub available: Vec<AvailablePackage>,
    /// The installed packages with a newer version, by name; those [`with_firmware`] too, which
    /// can't be chosen.
    pub upgrades: Vec<PackageUpgrade>,
}

/// The changes an admin asks for on one node: each list by name, run as one job (D-139).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, deny_unknown_fields)]
pub struct PackageChanges {
    pub install: Vec<String>,
    pub remove: Vec<String>,
    pub upgrade: Vec<String>,
}

impl PackageChanges {
    pub fn is_empty(&self) -> bool {
        self.install.is_empty() && self.remove.is_empty() && self.upgrade.is_empty()
    }

    /// Every name, in the lists' order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.install
            .iter()
            .chain(&self.remove)
            .chain(&self.upgrade)
            .map(String::as_str)
    }
}

/// Whether `name` may be handed to opkg or apk: a lowercase letter or digit, then letters,
/// digits, `.`, `_`, `+` and `-`, at most 128 bytes. Nothing that reads as an option, a
/// version constraint or a path.
pub fn valid_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    name.len() <= 128
        && (first.is_ascii_lowercase() || first.is_ascii_digit())
        && bytes.all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'+' | b'-')
        })
}

/// Packages essential on every node, by exact name or, ending in `*`, by prefix: the base
/// system, what uniwrt reaches and drives the node with (dropbear, ucode and its modules, the
/// package manager, uclient-fetch with its TLS), the network stack and the drivers.
const EVERYWHERE: &[&str] = &[
    "base-files",
    "busybox",
    "kernel",
    "kmod-*",
    "libc",
    "libgcc*",
    "procd*",
    "ubus",
    "ubusd",
    "libubus*",
    "libubox*",
    "libblobmsg-json*",
    "libjson-c*",
    "jshn",
    "jsonfilter",
    "uci",
    "libuci*",
    "netifd",
    "fstools",
    "libfstools*",
    "fwtool",
    "mtd",
    "logd",
    "urandom-seed",
    "urngd",
    "dropbear",
    "ucode",
    "ucode-mod-*",
    "libucode*",
    "apk-*",
    "opkg",
    "libopkg*",
    "uclient-fetch",
    "libuclient*",
    "libustream-*",
    "ca-bundle",
    "ca-certificates",
    "firewall4",
    "nftables*",
    "libnftnl*",
    "libmnl*",
    "dnsmasq*",
    "odhcpd*",
    "odhcp6c",
];
/// On a node with radios: what serves and meshes its Wi-Fi.
const RADIOS: &[&str] = &[
    "wpad*",
    "hostapd*",
    "iw",
    "iw-full",
    "iwinfo",
    "libiwinfo*",
    "wireless-regdb",
];
/// While the site meshes: batman-adv's tool (its kmod is a kmod).
const MESH: &[&str] = &["batctl*"];
/// On the gateway: the WAN's protocols.
const GATEWAY: &[&str] = &["ppp", "ppp-mod-pppoe", "ds-lite", "6rd"];
/// Never upgraded on their own: built with the firmware's kernel and C library.
const FIRMWARE: &[&str] = &["kernel", "kmod-*", "base-files", "libc"];

fn listed(list: &[&str], name: &str) -> bool {
    list.iter().any(|entry| match entry.strip_suffix('*') {
        Some(prefix) => name.starts_with(prefix),
        None => name == *entry,
    })
}

/// What a node is, as far as which packages it can't do without.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Needs {
    pub gateway: bool,
    pub radios: bool,
    pub mesh: bool,
}

/// Whether removing `name` from a node that `needs` it could cut it off or break its role.
pub fn essential(name: &str, needs: Needs) -> bool {
    listed(EVERYWHERE, name)
        || (needs.radios && listed(RADIOS, name))
        || (needs.mesh && listed(MESH, name))
        || (needs.gateway && listed(GATEWAY, name))
}

/// Whether `name` comes with the firmware: a new version must come with a firmware upgrade.
pub fn with_firmware(name: &str) -> bool {
    listed(FIRMWARE, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_package_names_pass() {
        for name in [
            "tcpdump",
            "luci-app-sqm",
            "libstdcpp6",
            "kmod-nf-nat6",
            "python3.11",
            "gtk+",
        ] {
            assert!(valid_name(name), "{name}");
        }
        for name in [
            "",
            "-f",
            "--force-depends",
            "Tcpdump",
            "a b",
            "a/b",
            "a=1",
            "a>1",
            "a;b",
            "!ppp",
            "~a",
        ] {
            assert!(!valid_name(name), "{name:?}");
        }
        assert!(!valid_name(&"a".repeat(129)));
    }

    #[test]
    fn essentials_follow_the_node() {
        let ap = Needs {
            radios: true,
            ..Needs::default()
        };
        for name in [
            "dropbear",
            "kmod-mt7915e",
            "ucode-mod-ubus",
            "apk-mbedtls",
            "libubox20260213",
            "procd-ujail",
        ] {
            assert!(essential(name, Needs::default()), "{name}");
        }
        for name in ["tcpdump", "luci", "etherwake", "ucodeish", "kmodx"] {
            assert!(!essential(name, ap), "{name}");
        }
        assert!(essential("wpad-mbedtls", ap));
        assert!(!essential("wpad-mbedtls", Needs::default()));
        assert!(essential(
            "batctl-default",
            Needs {
                mesh: true,
                ..Needs::default()
            }
        ));
        assert!(!essential("batctl-default", ap));
        assert!(essential(
            "ppp-mod-pppoe",
            Needs {
                gateway: true,
                ..Needs::default()
            }
        ));
        assert!(!essential("ppp-mod-pppoe", ap));
    }

    #[test]
    fn the_kernel_and_its_modules_come_with_the_firmware() {
        for name in ["kernel", "kmod-batman-adv", "base-files", "libc"] {
            assert!(with_firmware(name), "{name}");
        }
        for name in ["dnsmasq", "libcurl4", "luci-base"] {
            assert!(!with_firmware(name), "{name}");
        }
    }

    #[test]
    fn changes_name_every_package_and_reject_unknown_fields() {
        let changes: PackageChanges =
            serde_json::from_str(r#"{"install":["a"],"upgrade":["b"]}"#).unwrap();
        assert_eq!(changes.names().collect::<Vec<_>>(), ["a", "b"]);
        assert!(!changes.is_empty());
        assert!(PackageChanges::default().is_empty());
        assert!(serde_json::from_str::<PackageChanges>(r#"{"purge":["a"]}"#).is_err());
    }
}
