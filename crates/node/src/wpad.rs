//! The commands of the wpad swap (D-089): which package replaces which, the commands that download
//! them without changing anything, the one that swaps them on the node by itself, and what it
//! reports.
//!
//! Removing a wpad stops hostapd and `wpa_supplicant` (its `prerm` stops the `wpad` service) and
//! installing one starts them again, so a mesh AP reached over Wi-Fi loses its link in between.
//! Hence: every package is on the node before anything changes, the swap runs detached from the
//! login, offline, from those files, and it puts the old wpad back by itself when neither is
//! left installed. hostapd's access points and the mesh come back on their own once the service
//! runs again (checked on 24.10 and 25.12); the script brings Wi-Fi up where they don't.

use std::collections::BTreeSet;

use uniwrt_model::facts::PackageManager;

/// Where the packages, the swap's log and its result go, in RAM.
pub const DIR: &str = "/tmp/uniwrt-wpad";
/// The flash the swap needs free, in KiB: the full wpad installs ~800 KiB more than the basic
/// one (25.12, x86-64), and the package database grows.
pub const FLASH_KIB: u64 = 1536;
/// The RAM `/tmp` needs free, in KiB: three packages (~2 MiB) and apk's copy of the indexes.
pub const TMP_KIB: u64 = 8192;

/// The package both wpads depend on at the same version: downloaded too, in case the feed's
/// wpad is newer than the node's.
const COMMON: &str = "hostapd-common";
/// The TLS libraries a full wpad comes with, as its name's suffix.
const TLS: [&str; 3] = ["mbedtls", "wolfssl", "openssl"];
/// apk's names of the files in its cache, so `--no-network` installs them as the index's (a
/// package file named otherwise is untrusted).
const APK_CACHE_NAME: &str = "${name}-${version}.${hash:8}.apk";

/// One swap: `to` (the full wpad) in place of `from`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Swap {
    pub manager: PackageManager,
    pub from: String,
    pub to: String,
}

/// The swap the node's `installed` packages call for; `None` when its wpad is the full one, and
/// why not when there's nothing uniwrt can swap.
pub fn choose(
    manager: PackageManager,
    installed: &BTreeSet<String>,
) -> Result<Option<Swap>, String> {
    let wpads: Vec<&String> = installed
        .iter()
        .filter(|name| *name == "wpad" || name.starts_with("wpad-"))
        .collect();
    let [wpad] = wpads.as_slice() else {
        if wpads.is_empty() {
            let split = installed
                .iter()
                .any(|name| name.starts_with("hostapd-") && name != COMMON);
            return Err(if split {
                "hostapd and wpa_supplicant are installed as separate packages: swap them for wpad-mbedtls by hand".to_owned()
            } else {
                "No wpad package is installed".to_owned()
            });
        }
        let names: Vec<&str> = wpads.iter().map(|name| name.as_str()).collect();
        return Err(format!(
            "Several wpad packages are installed ({}): keep one",
            names.join(", ")
        ));
    };
    if *wpad == "wpad" || TLS.iter().any(|tls| **wpad == format!("wpad-{tls}")) {
        return Ok(None);
    }
    let tls = wpad
        .strip_prefix("wpad-basic-")
        .or_else(|| wpad.strip_prefix("wpad-mesh-"))
        .filter(|tls| TLS.contains(tls))
        .ok_or_else(|| format!("{wpad} has no full counterpart uniwrt knows"))?;
    Ok(Some(Swap {
        manager,
        from: (*wpad).clone(),
        to: format!("wpad-{tls}"),
    }))
}

fn argv(words: &[&str]) -> Vec<String> {
    words.iter().map(|&word| word.to_owned()).collect()
}

/// Downloads the full wpad, a copy of the current one (the swap's way back) and
/// [`COMMON`] into [`DIR`]; for apk, then checks that the swap resolves offline. Nothing
/// changes on the node.
pub fn fetch(swap: &Swap) -> Vec<Vec<String>> {
    let (to, from) = (swap.to.as_str(), swap.from.as_str());
    let mut commands = vec![argv(&["rm", "-rf", DIR]), argv(&["mkdir", "-p", DIR])];
    match swap.manager {
        PackageManager::Apk => {
            commands.extend([
                argv(&["apk", "--cache-dir", DIR, "update"]),
                argv(&[
                    "apk",
                    "--cache-dir",
                    DIR,
                    "fetch",
                    "-o",
                    DIR,
                    "--pkgname-spec",
                    APK_CACHE_NAME,
                    to,
                    from,
                    COMMON,
                ]),
                argv(&[
                    "apk",
                    "add",
                    "--no-network",
                    "--cache-dir",
                    DIR,
                    "--simulate",
                    to,
                    &format!("!{from}"),
                ]),
            ]);
        }
        PackageManager::Opkg => {
            commands.extend([
                crate::pkg::update(PackageManager::Opkg),
                // `opkg download` writes to the current directory, under the feed's file
                // names, which `--cache` looks for.
                argv(&["sh", "-c", DOWNLOAD, "sh", DIR, to, from, COMMON]),
            ]);
        }
    }
    commands
}

/// `opkg download` of each package after the first argument, in that directory.
const DOWNLOAD: &str = r#"cd "$1" || exit 1
shift
for p; do opkg download "$p" || exit 1; done"#;

/// Starts [`SWAP`] detached from the login (its own session where `setsid` exists, hangups
/// ignored, output to the log), so it carries on when the login drops with the Wi-Fi.
const LAUNCH: &str = r#"s=$1
shift
cd / || exit 1
rm -f "$4/state" "$4/log"
trap '' HUP PIPE
if command -v setsid >/dev/null 2>&1; then
	setsid sh -c "$s" sh "$@" </dev/null >"$4/log" 2>&1 &
else
	sh -c "$s" sh "$@" </dev/null >"$4/log" 2>&1 &
fi
echo started"#;

/// The swap, from the files in the directory and offline: `$1` the package manager, `$2` the
/// wpad installed, `$3` the full one, `$4` the directory. It writes one word to `state` when it
/// ends: `swapped`; `unchanged` (the old wpad still runs); `restored` (the old wpad is back
/// after the new one failed); `broken` (neither is installed).
const SWAP: &str = r#"trap '' HUP PIPE
m=$1 from=$2 to=$3 dir=$4
wifi_count() { ubus list 2>/dev/null | grep -cE '^(hostapd|wpa_supplicant)\.'; }
before=$(wifi_count)
put() {
	if [ "$m" = apk ]; then
		apk add --no-network --cache-dir "$dir" "$1" "!$2"
	else
		if opkg status "$2" | grep -q '^Status:.* installed'; then
			opkg remove "$2" || return 1
		fi
		opkg install --cache "$dir" "$1"
	fi
}
full() { [ -x /usr/sbin/hostapd ] && grep -qF wnm_sleep_mode /usr/sbin/hostapd; }
if put "$to" "$from" && full; then
	result=swapped
elif [ -x /usr/sbin/hostapd ]; then
	result=unchanged
elif put "$from" "$to" && [ -x /usr/sbin/hostapd ]; then
	result=restored
else
	result=broken
fi
echo "result: $result"
n=0
while [ "$(wifi_count)" -lt "$before" ] && [ "$n" -lt 30 ]; do
	sleep 1
	n=$((n + 1))
done
if [ "$(wifi_count)" -lt "$before" ]; then
	echo "bringing Wi-Fi up"
	wifi up
fi
rm -f "$dir"/*.apk "$dir"/*.ipk "$dir"/APKINDEX.*
echo "$result" >"$dir/state.new" && mv "$dir/state.new" "$dir/state""#;

/// The command that starts the swap; it answers `started` at once.
pub fn launch(swap: &Swap) -> Vec<String> {
    argv(&[
        "sh",
        "-c",
        LAUNCH,
        "sh",
        SWAP,
        crate::pkg::tool(swap.manager),
        &swap.from,
        &swap.to,
        DIR,
    ])
}

/// How the swap ended, from its `state` file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Swapped,
    Unchanged,
    Restored,
    Broken,
}

impl Outcome {
    pub fn parse(state: &str) -> Option<Self> {
        Some(match state.trim() {
            "swapped" => Self::Swapped,
            "unchanged" => Self::Unchanged,
            "restored" => Self::Restored,
            "broken" => Self::Broken,
            _ => return None,
        })
    }
}

/// The KiB available on the file system of a `df -k <path>` answer (BusyBox's or coreutils'; a
/// long device name may wrap its line).
pub fn available_kib(df: &str) -> Option<u64> {
    let fields: Vec<&str> = df.lines().last()?.split_whitespace().collect();
    // From the end: mount point, use%, available.
    fields
        .len()
        .checked_sub(3)
        .and_then(|i| fields[i].parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installed(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|&name| name.to_owned()).collect()
    }

    #[test]
    fn the_full_wpad_takes_the_basic_ones_place_with_the_same_tls_library() {
        let apk = PackageManager::Apk;
        let swap = |names: &[&str]| choose(apk, &installed(names));
        assert_eq!(
            swap(&["hostapd-common", "wpad-basic-mbedtls"]),
            Ok(Some(Swap {
                manager: apk,
                from: "wpad-basic-mbedtls".to_owned(),
                to: "wpad-mbedtls".to_owned(),
            }))
        );
        assert_eq!(
            swap(&["wpad-mesh-wolfssl"]).unwrap().unwrap().to,
            "wpad-wolfssl"
        );
        assert_eq!(swap(&["wpad-openssl"]), Ok(None));
        assert_eq!(swap(&["wpad"]), Ok(None));
        assert!(
            swap(&["wpad-mini"])
                .unwrap_err()
                .contains("no full counterpart")
        );
        assert!(
            swap(&["wpad-basic-mbedtls", "wpad-mbedtls"])
                .unwrap_err()
                .contains("Several")
        );
        assert!(
            swap(&["hostapd-common", "hostapd-basic-mbedtls"])
                .unwrap_err()
                .contains("separate packages")
        );
        assert_eq!(
            swap(&["hostapd-common"]),
            Err("No wpad package is installed".to_owned())
        );
    }

    #[test]
    fn fetching_downloads_both_wpads_and_changes_nothing() {
        let swap = |manager| Swap {
            manager,
            from: "wpad-basic-mbedtls".to_owned(),
            to: "wpad-mbedtls".to_owned(),
        };
        let apk = fetch(&swap(PackageManager::Apk));
        assert_eq!(apk[0], ["rm", "-rf", DIR]);
        assert_eq!(
            apk[3][8..],
            ["wpad-mbedtls", "wpad-basic-mbedtls", "hostapd-common"]
        );
        assert_eq!(
            apk[4],
            [
                "apk",
                "add",
                "--no-network",
                "--cache-dir",
                DIR,
                "--simulate",
                "wpad-mbedtls",
                "!wpad-basic-mbedtls"
            ]
        );
        let opkg = fetch(&swap(PackageManager::Opkg));
        assert_eq!(opkg[2], ["opkg", "update"]);
        assert_eq!(
            opkg[3][3..],
            [
                "sh",
                DIR,
                "wpad-mbedtls",
                "wpad-basic-mbedtls",
                "hostapd-common"
            ]
        );
        let launch = launch(&swap(PackageManager::Opkg));
        assert_eq!(
            launch[5..],
            ["opkg", "wpad-basic-mbedtls", "wpad-mbedtls", DIR]
        );
    }

    /// The scripts are POSIX sh: the local `sh` parses them.
    #[test]
    fn the_scripts_parse() {
        for script in [DOWNLOAD, LAUNCH, SWAP] {
            let status = std::process::Command::new("sh")
                .args(["-n", "-c", script])
                .status()
                .unwrap();
            assert!(status.success(), "{script}");
        }
    }

    #[test]
    fn outcomes_and_free_space_parse() {
        assert_eq!(Outcome::parse("swapped\n"), Some(Outcome::Swapped));
        assert_eq!(Outcome::parse("broken"), Some(Outcome::Broken));
        assert_eq!(Outcome::parse(""), None);
        let busybox = "Filesystem           1K-blocks      Used Available Use% Mounted on\n\
                       overlayfs:/overlay        4800      1220      3580  25% /\n";
        assert_eq!(available_kib(busybox), Some(3580));
        let wrapped = "Filesystem           1K-blocks      Used Available Use% Mounted on\n\
                       /dev/mapper/a-very-long-name\n\
                       \x20                       4800      1220      3580  25% /\n";
        assert_eq!(available_kib(wrapped), Some(3580));
        assert_eq!(available_kib("df: /nope: No such file"), None);
    }
}
