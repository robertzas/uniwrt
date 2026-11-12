//! The wpad swap's commands (D-089) on the hwsim lab (TESTING.md, Wi-Fi lab), as the job runs
//! them: node a (25.12, apk) and node b (24.10, opkg) get the basic wpad first, then the swap
//! downloads both wpads, loses the route to the feeds, and swaps offline. Once with the full
//! package corrupted, where the old wpad must still run when it ends; then for real, where the
//! agent finds 802.11v and every access point and mesh point is back. The lab ends as it was
//! (the full wpad) and needs the internet through Docker's bridge.

use std::{
    process::Command,
    thread,
    time::{Duration, Instant},
};

use uniwrt_model::facts::{PackageManager, WifiFeature};
use uniwrt_node::wpad::{self, Outcome, Swap};
use uniwrt_system::Node;

/// How long the swap may take, until its Wi-Fi is back.
const SWAP: Duration = Duration::from_secs(120);

/// Runs `argv` on `node`; panics with its output when it fails.
fn exec(node: &Node, command: &[String]) -> String {
    let words: Vec<&str> = command.iter().map(String::as_str).collect();
    node.sh(r#""$@""#, &words)
}

/// The hostapd and `wpa_supplicant` objects: the access points and mesh points running.
fn wifi_objects(node: &Node) -> Vec<String> {
    node.sh("ubus list", &[])
        .lines()
        .filter(|path| path.starts_with("hostapd.") || path.starts_with("wpa_supplicant."))
        .map(str::to_owned)
        .collect()
}

fn wait_for_objects(node: &Node, expected: &[String]) {
    let deadline = Instant::now() + SWAP;
    while wifi_objects(node) != expected {
        assert!(
            Instant::now() < deadline,
            "{}: Wi-Fi isn't back: {:?}, expected {expected:?}",
            node.container,
            wifi_objects(node)
        );
        thread::sleep(Duration::from_secs(1));
    }
}

fn has_wnm(node: &Node) -> bool {
    node.run("grep -qF wnm_sleep_mode /usr/sbin/hostapd", &[], &[])
        .ok
}

/// A default route through Docker's bridge and public resolvers, while `on`.
fn online(node: &Node, gateway: &str, on: bool) {
    if on {
        node.sh(
            r#"ip route replace default via "$1"
[ -e /tmp/resolv.conf.uniwrt ] || cp /tmp/resolv.conf /tmp/resolv.conf.uniwrt
printf 'nameserver 1.1.1.1\nnameserver 8.8.8.8\n' >/tmp/resolv.conf"#,
            &[gateway],
        );
    } else {
        node.sh(
            "ip route del default 2>/dev/null; mv /tmp/resolv.conf.uniwrt /tmp/resolv.conf 2>/dev/null; true",
            &[],
        );
    }
}

/// Launches the swap and waits for its result.
fn swap(node: &Node, swap: &Swap) -> Outcome {
    let started = exec(node, &wpad::launch(swap));
    assert_eq!(started.trim(), "started", "{}", node.container);
    let deadline = Instant::now() + SWAP;
    loop {
        let read = node.run(r#"cat "$1/state""#, &[wpad::DIR], &[]);
        if let Some(outcome) = read.ok.then(|| Outcome::parse(&read.stdout)).flatten() {
            return outcome;
        }
        assert!(
            Instant::now() < deadline,
            "{}: the swap didn't end: {}",
            node.container,
            node.sh(r#"cat "$1/log" 2>&1; true"#, &[wpad::DIR])
        );
        thread::sleep(Duration::from_secs(1));
    }
}

fn check(node: &Node, manager: PackageManager, gateway: &str) {
    let name = &node.container;
    let full = wifi_objects(node);
    assert!(!full.is_empty(), "{name}: the lab's Wi-Fi runs");

    // The basic wpad first, as a factory image has it.
    online(node, gateway, true);
    match manager {
        PackageManager::Apk => node.sh(
            "apk update >/dev/null && apk add wpad-basic-mbedtls '!wpad-mbedtls'",
            &[],
        ),
        PackageManager::Opkg => node.sh(
            "opkg update >/dev/null && opkg remove wpad-mbedtls && opkg install wpad-basic-mbedtls",
            &[],
        ),
    };
    wait_for_objects(node, &full);
    assert!(!has_wnm(node), "{name}: the basic wpad has no 802.11v");
    let facts = node.facts();
    let caps = facts.capabilities;
    assert!(!caps.wifi.contains(&WifiFeature::BssTransition), "{name}");
    let swap_to_full = Swap {
        manager,
        from: "wpad-basic-mbedtls".to_owned(),
        to: "wpad-mbedtls".to_owned(),
    };

    // The full package arrives corrupted: the basic wpad runs when the swap ends.
    for argv in wpad::fetch(&swap_to_full) {
        exec(node, &argv);
    }
    online(node, gateway, false);
    node.sh(
        r#"for f in "$1"/wpad-mbedtls*; do head -c 4096 "$f" >"$f.part" && mv "$f.part" "$f"; done"#,
        &[wpad::DIR],
    );
    let outcome = swap(node, &swap_to_full);
    assert!(
        matches!(outcome, Outcome::Unchanged | Outcome::Restored),
        "{name}: {outcome:?}: {}",
        node.sh(r#"cat "$1/log""#, &[wpad::DIR])
    );
    wait_for_objects(node, &full);
    assert!(!has_wnm(node), "{name}");

    // For real, offline once fetched.
    online(node, gateway, true);
    for argv in wpad::fetch(&swap_to_full) {
        exec(node, &argv);
    }
    online(node, gateway, false);
    let outcome = swap(node, &swap_to_full);
    assert_eq!(
        outcome,
        Outcome::Swapped,
        "{name}: {}",
        node.sh(r#"cat "$1/log""#, &[wpad::DIR])
    );
    wait_for_objects(node, &full);
    assert!(
        node.facts()
            .capabilities
            .wifi
            .contains(&WifiFeature::BssTransition),
        "{name}: the agent finds 802.11v"
    );
    // The packages and indexes are gone; the result and the log stay for the job.
    let left = node.sh(r#"ls "$1""#, &[wpad::DIR]);
    assert_eq!(
        left.split_whitespace().collect::<Vec<_>>(),
        ["log", "state"]
    );
    node.sh(r#"rm -rf "$1""#, &[wpad::DIR]);
}

#[test]
#[ignore = "needs the hwsim lab"]
fn the_full_wpad_swaps_in_offline_and_falls_back() {
    let output = Command::new("docker")
        .args([
            "network",
            "inspect",
            "bridge",
            "-f",
            "{{(index .IPAM.Config 0).Gateway}}",
        ])
        .output()
        .expect("docker runs");
    let gateway = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    assert!(!gateway.is_empty(), "Docker's bridge has a gateway");
    for (container, manager) in [
        ("uniwrt-wifi-a", PackageManager::Apk),
        ("uniwrt-wifi-b", PackageManager::Opkg),
    ] {
        let node = Node::new(container);
        check(&node, manager, &gateway);
        online(&node, &gateway, false);
    }
}
