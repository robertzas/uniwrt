//! The support bundle's snapshot on a simulated node (D-144): the fixed script's sections as
//! the real one prints them, with a Wi-Fi status whose keys the bundle must mask.

use std::fmt::Write as _;

use serde_json::json;
use uniwrt_node::ExecOutput;

use super::commands::ok;
use crate::world::Node;

/// What `uniwrt_node::support::SCRIPT` prints on `node`.
pub(crate) fn snapshot(node: &Node) -> ExecOutput {
    let board = super::system::board(node)["board"].clone();
    let wireless = json!({
        "radio0": {
            "up": true,
            "interfaces": [{
                "ifname": "phy0-ap0",
                "config": { "mode": "ap", "ssid": "uniwrt-demo", "key": "sim-wifi-passphrase" },
            }],
        },
    });
    let mut text = String::new();
    let sections = [
        ("board.json", board.to_string()),
        ("system.json", json!({ "uptime": 3600 }).to_string()),
        ("wireless.json", wireless.to_string()),
        ("interfaces.json", json!({ "interface": [] }).to_string()),
        (
            "addresses.txt",
            format!("2: br-lan    inet {}/24\n", node.ip),
        ),
        (
            "routes.txt",
            "default via 192.168.1.1 dev br-lan\n".to_owned(),
        ),
        ("routes6.txt", String::new()),
        ("neighbors.txt", String::new()),
        ("bridges.txt", String::new()),
        (
            "disk.txt",
            "Filesystem Size Used Available Use% Mounted on\n".to_owned(),
        ),
        ("processes.txt", "PID USER COMMAND\n".to_owned()),
        (
            "dmesg.txt",
            "[    0.000000] Linux version 6.12\n".to_owned(),
        ),
        (
            "logread.txt",
            format!("daemon.info {}: booted\n", node.name),
        ),
    ];
    for (name, body) in sections {
        let _ = write!(text, "\n=== {name}\n{body}\n");
    }
    ok(text.into_bytes())
}
