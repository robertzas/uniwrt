//! Rendered Wi-Fi and mesh on the hwsim lab (TESTING.md, System; scripts/wifi-lab.sh): node a
//! becomes the gateway and node b the AP of [`site::lab`], and client c must lease again on
//! both bands. b's 5 GHz uplink reaches a's DHCP server only through the mesh: b is cut off
//! Docker's network for the test, so its frames cross `bat0`, tagged with labnet's VLAN.
//! Then c's 5 GHz uplink fast-roams (802.11r with SAE, D-085) from b to a and back: each
//! access point reports the station authorized by FT, the keys handed between them under the
//! rendered key holders. Before that, the agent's `rrm` op gives each 5 GHz access point the
//! other as its 802.11k neighbor. The lab's cable `bh0` comes over to the wired backhaul
//! (D-094; `wifi/backhaul.rs`): a trunk with batman-adv's VLAN beside it at first, then the VLAN
//! alone. The test leaves the lab rendered; the script restarts it afterwards.

#[path = "wifi/backhaul.rs"]
mod backhaul;

use std::{
    process::Command,
    thread,
    time::{Duration, Instant},
};

use serde_json::json;
use uniwrt_core::{Ipv4Net, MacAddr};
use uniwrt_model::site::SiteConfig;
use uniwrt_render::BackhaulStage;
use uniwrt_system::{Node, ipv4, site};

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
    iw_dev(node)
        .into_iter()
        .filter(|iface| iface.mode == mode && !iface.name.is_empty())
        .map(|iface| iface.name)
        .collect()
}

/// An interface as `iw dev` lists it.
#[derive(Default)]
struct Iface {
    name: String,
    addr: String,
    mode: String,
    channel: String,
}

fn iw_dev(node: &Node) -> Vec<Iface> {
    let out = node.sh("iw dev", &[]);
    let mut found: Vec<Iface> = Vec::new();
    for line in out.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix("Interface ") {
            found.push(Iface {
                name: name.to_owned(),
                ..Iface::default()
            });
        } else if line.starts_with("Unnamed") || line.starts_with("phy#") {
            // A P2P device without a netdev, or the next phy: not the interface before.
            found.push(Iface::default());
        } else if let Some(iface) = found.last_mut() {
            if let Some(addr) = line.strip_prefix("addr ") {
                addr.clone_into(&mut iface.addr);
            } else if let Some(mode) = line.strip_prefix("type ") {
                mode.clone_into(&mut iface.mode);
            } else if let Some(channel) = line.strip_prefix("channel ") {
                channel
                    .split(' ')
                    .next()
                    .unwrap_or_default()
                    .clone_into(&mut iface.channel);
            }
        }
    }
    found
}

/// The node's access point on channel 36, once it is up there: its interface and BSSID.
fn ap_on_36(node: &Node) -> (String, String) {
    let find = || {
        iw_dev(node)
            .into_iter()
            .find(|iface| iface.mode == "AP" && iface.channel == "36" && !iface.name.is_empty())
    };
    wait(
        "an access point on channel 36",
        node,
        || find().is_some(),
        "iw dev",
    );
    let iface = find().expect("an access point on channel 36");
    (iface.name, iface.addr)
}

/// What c's `wpa_supplicant` on `iface` answers `command` (its ubus `control`); nothing while
/// the interface is (re)starting.
fn supplicant(c: &Node, iface: &str, command: &str) -> String {
    let run = c.run(
        r#"ubus call "wpa_supplicant.$1" control "{\"command\":\"$2\"}""#,
        &[iface, command],
        &[],
    );
    serde_json::from_str::<serde_json::Value>(&run.stdout)
        .ok()
        .and_then(|reply| reply["result"].as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// A `key=value` line of `STATUS`.
fn status_of(status: &str, key: &str) -> Option<String> {
    status
        .lines()
        .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
        .map(str::to_owned)
}

/// c's station `iface` moves to `to`'s access point on channel 36 with an FT authentication,
/// which `to`'s hostapd reports for `station`.
fn fast_roam(c: &Node, iface: &str, station: MacAddr, to: &Node) {
    let (ap, bssid) = ap_on_36(to);
    to.sh(
        r#"ubus subscribe "hostapd.$1" </dev/null >/tmp/ft.log 2>&1 & echo $! >/tmp/ft.pid"#,
        &[&ap],
    );
    let context = "logread | grep -E 'wpa_supplicant|hostapd' | tail -n 30";
    wait(
        "c's scan to find the target",
        c,
        || {
            supplicant(c, iface, "SCAN");
            thread::sleep(Duration::from_secs(4));
            supplicant(c, iface, "SCAN_RESULTS").contains(&bssid)
        },
        context,
    );
    assert_eq!(supplicant(c, iface, &format!("ROAM {bssid}")).trim(), "OK");
    wait(
        "c's roam",
        c,
        || {
            let status = supplicant(c, iface, "STATUS");
            status_of(&status, "bssid").as_deref() == Some(bssid.as_str())
                && status_of(&status, "wpa_state").as_deref() == Some("COMPLETED")
        },
        context,
    );
    let status = supplicant(c, iface, "STATUS");
    assert_eq!(status_of(&status, "key_mgmt").as_deref(), Some("FT-SAE"));
    let events: String = to
        .sh("kill $(cat /tmp/ft.pid); cat /tmp/ft.log", &[])
        .split_whitespace()
        .collect();
    let authorized = format!(r#""address":"{station}","ifname":"{ap}","auth-alg":"ft""#);
    assert!(
        events.contains(&authorized),
        "{}: no FT authorization of {station} in {events}",
        to.container
    );
}

/// The agent's `rrm` op on the rendered 802.11k (features/wifi.md, Roaming): each node's 5 GHz
/// access point reports itself and takes the other's as its neighbor, which hostapd's list then
/// holds; the same lists sent again change nothing.
fn neighbor_reports(a: &Node, b: &Node) {
    let mine = |node: &Node| {
        let (ifname, _) = ap_on_36(node);
        let state = node.agent("rrm", &json!({})).expect("rrm answers");
        let own = state["own"].as_array().cloned().unwrap_or_default();
        let report = own.into_iter().find(|o| o["ifname"] == ifname.as_str());
        let report =
            report.unwrap_or_else(|| panic!("{}: no report of {ifname}: {state}", node.container));
        (ifname, report)
    };
    let (ra, rb) = (mine(a), mine(b));
    for (node, (ifname, _), (_, theirs)) in [(a, &ra, &rb), (b, &rb, &ra)] {
        let entry = json!([theirs["bssid"], theirs["ssid"], theirs["nr"]]);
        let lists = json!({"lists": {ifname: [entry]}});
        let state = node.agent("rrm", &lists).expect("rrm sets");
        assert_eq!(state["set"], json!([ifname]), "{}: {state}", node.container);
        assert_eq!(state["failed"], json!([]), "{}: {state}", node.container);
        let held = node.sh(r#"ubus call "hostapd.$1" rrm_nr_list"#, &[ifname]);
        let bssid = theirs["bssid"].as_str().expect("a BSSID");
        assert!(held.contains(bssid), "{}: {held}", node.container);
        let again = node.agent("rrm", &lists).expect("rrm answers");
        assert_eq!(
            again["set"],
            json!([]),
            "{}: held already: {again}",
            node.container
        );
    }
}

/// `node`'s 802.11s peer link is established, batman-adv sees the other node over it and over
/// the cable, and the bridge carries the networks as rendered.
fn meshed(site: &SiteConfig, node: &Node) {
    let context = "iw dev; for i in $(iw dev | awk '/Interface/ { print $2 }'); do iw dev $i station dump; done; batctl meshif bat0 n; logread | tail -n 40";
    let mesh = || ifaces(node, "mesh point");
    wait(
        "the 802.11s peer link",
        node,
        || {
            mesh().iter().any(|mesh| {
                node.sh(r#"iw dev "$1" station dump"#, &[mesh])
                    .contains("ESTAB")
            })
        },
        context,
    );
    wait(
        "a batman neighbor over the mesh point",
        node,
        || {
            let neighbors = node.sh("batctl meshif bat0 n -H", &[]);
            mesh().iter().any(|mesh| neighbors.contains(mesh.as_str()))
        },
        context,
    );
    // Wi-Fi joins br-lan untagged in its network's VLAN; bat0 is a trunk.
    for iface in ifaces(node, "AP") {
        bridged(node, &iface, &["99PU"]);
    }
    bridged(node, "bat0", &["1PU", "99"]);
    // The cable: a trunk, and batman-adv's VLAN beside it.
    bridged(node, backhaul::PORT, &["1PU", "99"]);
    backhaul::neighbor_over_cable(site, node);
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

    let (a_facts, b_facts) = backhaul::facts(&a, &b);
    let (gateway, ap) = backhaul::render(
        &site,
        (&a, &a_facts),
        (&b, &b_facts),
        BackhaulStage::Overlay,
    );
    let status = Command::new("docker")
        .args(["network", "disconnect", "bridge", "uniwrt-wifi-b"])
        .status()
        .expect("docker runs");
    assert!(status.success(), "disconnecting b from Docker's network");
    a.apply(&gateway);
    b.apply(&ap);

    for node in [&a, &b] {
        meshed(&site, node);
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

    neighbor_reports(&a, &b);

    // c's 5 GHz uplink offers 802.11r too, joins b with FT-SAE, then roams to a and back.
    c.sh(
        "uci set wireless.sta_radio1.ieee80211r=1 && uci commit wireless && wifi reload",
        &[],
    );
    let station: MacAddr = "02:00:00:00:05:00".parse().expect("a MAC");
    let find = || {
        c.run("ubus list 'wpa_supplicant.*'", &[], &[])
            .stdout
            .lines()
            .filter_map(|path| path.strip_prefix("wpa_supplicant."))
            .find(|iface| supplicant(&c, iface, "STATUS").contains(&format!("address={station}")))
            .map(str::to_owned)
    };
    wait(
        "c's 5 GHz station",
        &c,
        || find().is_some(),
        "ubus list; logread | tail -n 20",
    );
    let iface = find().expect("c's 5 GHz station");

    let (_, b_bssid) = ap_on_36(&b);
    wait(
        "c's FT-SAE association with b",
        &c,
        || {
            let status = supplicant(&c, &iface, "STATUS");
            status_of(&status, "bssid").as_deref() == Some(b_bssid.as_str())
                && status_of(&status, "key_mgmt").as_deref() == Some("FT-SAE")
                && status_of(&status, "wpa_state").as_deref() == Some("COMPLETED")
        },
        "logread | grep wpa_supplicant | tail -n 30",
    );
    fast_roam(&c, &iface, station, &a);
    fast_roam(&c, &iface, station, &b);

    backhaul::exclusive(&site, (&a, &a_facts), (&b, &b_facts));
}
