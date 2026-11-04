//! Rendered Wi-Fi and mesh on the hwsim lab (TESTING.md, System; scripts/wifi-lab.sh): node a
//! becomes the gateway and node b the AP of [`site::lab`], and client c must lease again on
//! both bands. b's 5 GHz uplink reaches a's DHCP server only through the mesh: b is cut off
//! Docker's network for the test, so its frames cross `bat0`, tagged with labnet's VLAN.
//! The test leaves the lab rendered; the script restarts it afterwards.

use std::{
    process::Command,
    thread,
    time::{Duration, Instant},
};

use uniwrt_core::Ipv4Net;
use uniwrt_model::node::NodeRole;
use uniwrt_system::{
    Node, ipv4, render_for, render_with,
    site::{self, AP, GATEWAY},
};

const UP: Duration = Duration::from_secs(90);

/// Polls `check` until it holds, or panics with what `context` prints then.
fn wait(what: &str, node: &Node, check: impl Fn() -> bool, context: &str) {
    let start = Instant::now();
    while !check() {
        assert!(
            start.elapsed() < UP,
            "{what} did not happen in {UP:?}:\n{}",
            node.sh(context, &[])
        );
        thread::sleep(Duration::from_secs(1));
    }
}

/// The bridge port `iface` is in exactly `vlans` (`bridge vlan show`).
fn bridged(node: &Node, iface: &str, vlans: &[&str]) {
    let ports = node.bridge_vlans();
    let want: Vec<String> = vlans.iter().map(|v| (*v).to_owned()).collect();
    assert_eq!(
        ports.get(iface),
        Some(&want),
        "{}: {iface} in {ports:#?}",
        node.container
    );
}

/// The interfaces of `mode` (`AP`, `mesh point`) that `iw dev` lists.
fn ifaces(node: &Node, mode: &str) -> Vec<String> {
    let out = node.sh("iw dev", &[]);
    let mut names = Vec::new();
    let mut current = "";
    for line in out.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix("Interface ") {
            current = name;
        } else if line == format!("type {mode}") {
            names.push(current.to_owned());
        }
    }
    names
}

#[test]
#[ignore = "needs the hwsim lab"]
fn rendered_wlans_and_mesh_carry_the_client() {
    let (a, b, c) = (
        Node::new("uniwrt-wifi-a"),
        Node::new("uniwrt-wifi-b"),
        Node::new("uniwrt-wifi-c"),
    );
    // The site's LAN keeps a's address on Docker's network, where the tests reach it.
    let lan = a.interface("lan").expect("a has a lan");
    let address = ipv4(&lan).pop().expect("lan has an address");
    let lan: Ipv4Net = address.parse().expect("an address/mask");
    let site = site::lab(lan);

    // The lab's gateway has no WAN port: a veth stub stands in for one.
    a.sh("ip link add eth1 type veth peer name eth1-isp", &[]);
    let mut facts = a.facts();
    facts.board.layout.wan = Some("eth1".to_owned());
    let gateway = render_with(&site, &a, GATEWAY, NodeRole::Gateway, &facts);
    let ap = render_for(&site, &b, AP, NodeRole::Ap);
    let status = Command::new("docker")
        .args(["network", "disconnect", "bridge", "uniwrt-wifi-b"])
        .status()
        .expect("docker runs");
    assert!(status.success(), "disconnecting b from Docker's network");
    a.apply(&gateway);
    b.apply(&ap);

    let mesh_up = "iw dev; for i in $(iw dev | awk '/Interface/ { print $2 }'); do iw dev $i station dump; done; batctl meshif bat0 n; logread | tail -n 40";
    for node in [&a, &b] {
        wait(
            "the 802.11s peer link",
            node,
            || {
                ifaces(node, "mesh point").iter().any(|mesh| {
                    node.sh(r#"iw dev "$1" station dump"#, &[mesh])
                        .contains("ESTAB")
                })
            },
            mesh_up,
        );
        wait(
            "a batman neighbor",
            node,
            || !node.sh("batctl meshif bat0 n -H", &[]).trim().is_empty(),
            mesh_up,
        );
        // Wi-Fi joins br-lan untagged in its network's VLAN; bat0 is a trunk.
        for iface in ifaces(node, "AP") {
            bridged(node, &iface, &["99PU"]);
        }
        bridged(node, "bat0", &["1PU", "99"]);
    }

    // c leases again on both uplinks, the 5 GHz one through b and the mesh.
    c.sh("ifup wwan; ifup wwan5", &[]);
    // The lab's own config leased on br-mesh: an ACK on br-lan.99 comes from the rendered one.
    let acks = "logread | grep -E 'dnsmasq-dhcp|hostapd' | tail -n 40";
    for mac in ["02:00:00:00:04:00", "02:00:00:00:05:00"] {
        wait(
            "c's lease on labnet",
            &a,
            || {
                a.sh("logread | grep 'DHCPACK(br-lan.99)' || true", &[])
                    .contains(mac)
            },
            acks,
        );
    }
    for uplink in ["wwan", "wwan5"] {
        c.wait_interface(uplink, UP, |s| {
            ipv4(s).iter().any(|a| a.starts_with("10.99.0."))
        });
    }
}
