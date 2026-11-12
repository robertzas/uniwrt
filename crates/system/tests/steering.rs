//! Steering's usteer (D-090) on the hwsim lab: node a (25.12, apk) and node b (24.10, opkg)
//! install usteer from the feeds, take the `usteer` package rendered for the lab site with the
//! Balanced profile, and run it: each reads back the profile's thresholds and the steered SSID
//! (`get_config`), steers its access points of that SSID (`local_info`), and hears the other's
//! over the LAN (`remote_info`). The lab ends as it was: usteer removed, since it would take
//! over the neighbor lists the other tests check. Needs the internet through Docker's bridge.

use std::{
    process::Command,
    thread,
    time::{Duration, Instant},
};

use serde_json::Value;
use uniwrt_core::{Ipv4Net, NodeId};
use uniwrt_model::{
    facts::PackageManager,
    node::NodeRole,
    site::{node::ManagedPackage, steering::SteeringProfile},
};
use uniwrt_system::{Node, ipv4, render_for, site};

/// How long usteer may take to hear the other node.
const HEAR: Duration = Duration::from_secs(60);

/// A default route through Docker's bridge and public resolvers, while `on` (as the wpad test).
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

fn ubus(node: &Node, method: &str) -> Value {
    let out = node.sh(r#"ubus call usteer "$1""#, &[method]);
    serde_json::from_str(&out)
        .unwrap_or_else(|e| panic!("{}: {method}: {e}: {out}", node.container))
}

/// The SSIDs of a table of BSSes (`local_info`, `remote_info`).
fn ssids(nodes: &Value) -> Vec<String> {
    nodes
        .as_object()
        .into_iter()
        .flat_map(|nodes| nodes.values())
        .filter_map(|node| node["ssid"].as_str().map(str::to_owned))
        .collect()
}

fn docker_gateway() -> String {
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
    gateway
}

#[test]
#[ignore = "needs the hwsim lab"]
fn rendered_usteer_steers_the_lab_ssid_and_hears_the_other_node() {
    let gateway = docker_gateway();
    let nodes = [
        (
            Node::new("uniwrt-wifi-a"),
            PackageManager::Apk,
            NodeId::new(1),
        ),
        (
            Node::new("uniwrt-wifi-b"),
            PackageManager::Opkg,
            NodeId::new(2),
        ),
    ];
    let lan = nodes[0].0.interface("lan").expect("a has a lan");
    let address = ipv4(&lan).pop().expect("lan has an address");
    let lan: Ipv4Net = address.parse().expect("an address/mask");
    let mut site = site::lab(lan);
    site.steering.profile = SteeringProfile::Balanced;
    let ssid = site.wlans[0].ssid.clone();

    for (node, manager, id) in &nodes {
        online(node, &gateway, true);
        let install = match manager {
            PackageManager::Apk => "apk update >/dev/null && apk add usteer",
            PackageManager::Opkg => "opkg update >/dev/null && opkg install usteer",
        };
        node.sh(install, &[]);
        online(node, &gateway, false);
        // Only usteer's package: the lab's own wireless stays as it is.
        let mut rendered = render_for(&site, node, *id, NodeRole::Ap);
        assert_eq!(rendered.services.get("usteer"), Some(&true));
        rendered
            .packages
            .retain(|package, _| *package == ManagedPackage::Usteer);
        rendered.services.retain(|service, _| *service == "usteer");
        node.apply(&rendered);
    }

    for (node, ..) in &nodes {
        let config = ubus(node, "get_config");
        assert_eq!(
            config["roam_trigger_snr"], -75,
            "{}: {config}",
            node.container
        );
        assert_eq!(
            config["signal_diff_threshold"], 8,
            "{}: {config}",
            node.container
        );
        assert_eq!(
            config["ssid_list"],
            serde_json::json!([ssid]),
            "{}",
            node.container
        );
        let local = ssids(&ubus(node, "local_info"));
        assert!(
            !local.is_empty() && local.iter().all(|s| *s == ssid),
            "{}: usteer steers {local:?}",
            node.container
        );
    }
    for (node, ..) in &nodes {
        let deadline = Instant::now() + HEAR;
        loop {
            let remote = ubus(node, "remote_info");
            // One table of every remote BSS, keyed `<host>#hostapd.<ifname>`.
            let heard = ssids(&remote);
            if heard.contains(&ssid) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "{}: usteer hears no other node: {remote}",
                node.container
            );
            thread::sleep(Duration::from_secs(2));
        }
    }

    for (node, manager, _) in &nodes {
        let remove = match manager {
            PackageManager::Apk => "apk del usteer",
            PackageManager::Opkg => "opkg remove usteer",
        };
        node.sh(
            &format!("/etc/init.d/usteer stop; /etc/init.d/usteer disable; {remove}; rm -f /etc/config/usteer"),
            &[],
        );
    }
}
