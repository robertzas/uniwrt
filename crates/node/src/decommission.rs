//! The command that takes a node out of the site (features/lifecycle.md, Replace / RMA /
//! decommission; D-131): it leaves the mesh, the uniwrt footprint and the controller's key go,
//! and optionally `firstboot` resets the node to its image's defaults and restarts it.
//!
//! Leaving the mesh may cut the login that runs it (an AP reached over the mesh), so the wipe
//! runs detached from it, as the wpad swap does ([`crate::wpad`]), a second after answering
//! `started`; nothing reports back afterwards, since the controller's key is gone. The
//! transaction tool is removed before, by `txn uninstall` ([`crate::txn::uninstall`]), which
//! refuses while a transaction is open.

/// Where the wipe's log goes, in RAM (outside `/tmp/uniwrt`, which it removes).
pub const LOG: &str = "/tmp/uniwrt-decommission.log";

/// Starts [`WIPE`] in its own session where `setsid` exists, hangups ignored, its output to
/// `$2`; answers `started`.
const LAUNCH: &str = r#"s=$1
log=$2
shift 2
cd / || exit 1
trap '' HUP PIPE
if command -v setsid >/dev/null 2>&1; then
	setsid sh -c "$s" sh "$@" </dev/null >"$log" 2>&1 &
else
	sh -c "$s" sh "$@" </dev/null >"$log" 2>&1 &
fi
echo started"#;

/// The wipe: `$1` the controller's `authorized_keys` line, `$2` `1` to run `firstboot`. Section
/// names come from `uci show`, which prints only `[A-Za-z0-9_]` names, and the pattern keeps
/// only those.
const WIPE: &str = r#"trap '' HUP PIPE
sleep 1
hello=/etc/init.d/uniwrt-hello
guard=/etc/init.d/uniwrt-guard
for s in "$hello" "$guard"; do
	if [ -x "$s" ]; then
		"$s" disable
		"$s" stop >/dev/null 2>&1
	fi
done
rm -f "$hello" "$guard" /usr/sbin/uniwrt-hello /lib/upgrade/keep.d/uniwrt
rm -rf /etc/uniwrt /tmp/uniwrt /tmp/uniwrt.sh /var/run/uniwrt-lend
f=/etc/dropbear/authorized_keys
if [ -f "$f" ]; then
	grep -vxF "$1" "$f" >"$f.uniwrt"
	cat "$f.uniwrt" >"$f"
	rm -f "$f.uniwrt"
fi
echo "footprint removed"
if [ "$2" = 1 ]; then
	sync
	firstboot -y && reboot
	exit
fi
changed=
for s in $(uci -q show wireless | sed -n "s/^wireless\.\([A-Za-z0-9_]*\)\.mode='mesh'$/\1/p"); do
	uci -q delete "wireless.$s" && changed=1
done
for s in $(uci -q show network | sed -n "s/^network\.\([A-Za-z0-9_]*\)\.proto='batadv_hardif'$/\1/p"); do
	uci -q delete "network.$s" && changed=1
done
if [ -n "$changed" ]; then
	uci commit wireless
	uci commit network
	reload_config
	echo "left the mesh"
fi"#;

/// The command that starts the wipe; it answers `started` at once (check with
/// [`started`]).
pub fn wipe(controller_key: &str, firstboot: bool) -> Vec<String> {
    [
        "sh",
        "-c",
        LAUNCH,
        "sh",
        WIPE,
        LOG,
        controller_key.trim(),
        if firstboot { "1" } else { "0" },
    ]
    .map(str::to_owned)
    .to_vec()
}

/// Whether the launch's output says the wipe started.
pub fn started(output: &str) -> bool {
    output.trim() == "started"
}

#[cfg(test)]
mod tests {
    use std::{fs, process::Command};

    use super::*;

    #[test]
    fn the_scripts_parse() {
        for script in [LAUNCH, WIPE] {
            let status = Command::new("sh")
                .args(["-n", "-c", script])
                .status()
                .unwrap();
            assert!(status.success(), "{script}");
        }
    }

    #[test]
    fn the_wipe_takes_the_key_and_its_arguments_stay_arguments() {
        let request = wipe("ssh-ed25519 AAAA uniwrt\n", true);
        assert_eq!(request[..4], ["sh", "-c", LAUNCH, "sh"]);
        assert_eq!(request[4..], [WIPE, LOG, "ssh-ed25519 AAAA uniwrt", "1"]);
        assert_eq!(wipe("k", false)[7], "0");
        assert!(started("started\n"));
        assert!(!started(""));
    }

    /// The key line goes and every other line stays, whatever it holds: the script's part that
    /// edits `authorized_keys`, run on a copy.
    #[test]
    fn only_the_controllers_key_line_leaves_authorized_keys() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("authorized_keys");
        let key = "ssh-ed25519 AAAA uniwrt";
        fs::write(&file, format!("ssh-rsa BBBB admin\n{key}\n{key} extra\n")).unwrap();
        let start = WIPE.find("f=/etc/dropbear").unwrap();
        let end = WIPE.find("echo \"footprint removed\"").unwrap();
        let part = WIPE[start..end].replace("/etc/dropbear/authorized_keys", "$3");
        let status = Command::new("sh")
            .args(["-c", &part, "sh", key, "0"])
            .arg(&file)
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            format!("ssh-rsa BBBB admin\n{key} extra\n")
        );
    }
}
