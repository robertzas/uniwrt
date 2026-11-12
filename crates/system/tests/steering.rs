//! Steering's usteer (D-090) on the hwsim lab: node a (25.12, apk) and node b (24.10, opkg)
//! install usteer from the feeds, take the `usteer` package rendered for the lab site with the
//! Balanced profile, and run it: each reads back the profile's thresholds and the steered SSID
//! (`get_config`), steers its access points of that SSID (`local_info`), and hears the other's
//! over the LAN (`remote_info`), and a's usteer places client c on its access point
//! (`get_client_info`, as the controller's steering reads it). On a, the working tree's agent, subscribed to `usteer` and
//! `hostapd`, pushes what steering does to client c (the steering timeline): a transition
//! request's answer, and a signal kick under a minimum above c's signal, which takes c from the
//! disconnect that follows. hostapd refuses a transition request without the MBO fields, as
//! usteer 2022.08 sends them, on both releases (features/wifi.md, Steering & balancing).
//! The lab ends as it was: c back on a, usteer removed, since it would take over the neighbor
//! lists the other tests check. Needs the internet through Docker's bridge.

use std::{
    process::Command,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, value::RawValue};
use uniwrt_core::{Ipv4Net, NodeId};
use uniwrt_model::{
    event::{NodeEvent, SteerAction, SteerEvent},
    facts::PackageManager,
    node::NodeRole,
    site::{node::ManagedPackage, steering::SteeringProfile},
};
use uniwrt_node::{
    AgentEvent,
    events::{self, Push},
};
use uniwrt_system::{Node, ipv4, render_for, site};

/// Client c's 2.4 GHz uplink, which a serves.
const CLIENT: &str = "02:00:00:00:04:00";
/// How long c may take to come back after a kick.
const BACK: Duration = Duration::from_secs(90);

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

/// The access point of `node` that `CLIENT` is connected to.
fn serving(node: &Node) -> Option<String> {
    node.sh(
        r#"for o in $(ubus list 'hostapd.*'); do
  ubus call "$o" get_clients | grep -q "\"$1\"" && echo "${o#hostapd.}"
done; true"#,
        &[CLIENT],
    )
    .lines()
    .next()
    .map(str::to_owned)
}

/// usteer's view of `CLIENT`, read as the engine's steering reads it (D-093): `node`'s access
/// point serving it, with its signal.
fn client_info_places_the_client(node: &Node) {
    let iface = serving(node).unwrap_or_else(|| panic!("{}: c isn't connected", node.container));
    let out = node.sh(
        r#"ubus call usteer get_client_info "$(printf '{"address":"%s"}' "$1")""#,
        &[CLIENT],
    );
    let raw: Box<RawValue> = serde_json::from_str(&out)
        .unwrap_or_else(|e| panic!("{}: get_client_info: {e}: {out}", node.container));
    let heard = uniwrt_node::proto::ops::usteer_heard(&raw).expect("usteer's client info");
    assert!(
        heard.iter().any(|h| h.host.is_none() && h.iface == iface),
        "{}: usteer doesn't place c on {iface}: {out}",
        node.container
    );
}

/// A transition request to `CLIENT` at `iface`, towards `node`'s other access points: with the
/// MBO fields (`mbo`), or as usteer 2022.08 sends it. Whether hostapd sent it.
fn transition_request(node: &Node, iface: &str, mbo: bool) -> bool {
    let script = r#"set -e
o="hostapd.$1"
nr=$(for other in $(ubus list 'hostapd.*'); do
  [ "$other" = "$o" ] || ubus call "$other" rrm_nr_get_own | jsonfilter -e '@.value[2]'
done | head -n1)
extra=''
[ "$3" = mbo ] && extra=',"mbo_reason":0,"cell_pref":255,"reassoc_delay":0'
ubus call "$o" bss_transition_request \
  "{\"addr\":\"$2\",\"dialog_token\":1,\"validity_period\":100,\"neighbors\":[\"$nr\"]$extra}""#;
    node.run(script, &[iface, CLIENT, if mbo { "mbo" } else { "-" }], &[])
        .ok
}

/// What the agent on `node` pushes while usteer steers `CLIENT` at `iface`: a transition
/// request's answer, then a signal kick.
fn steered(node: &Node, iface: &str) -> Vec<SteerEvent> {
    let script = r#"
{
  echo '{"id":1,"op":"subscribe","args":{"topics":["hostapd","usteer"]}}'
  sleep 2
  ubus call "hostapd.$1" bss_transition_request "$2" >/dev/null
  sleep 3
  ubus call usteer update_config '{"min_snr":-1,"min_snr_kick_delay":1000}' >/dev/null
  sleep 6
  ubus call usteer update_config '{"min_snr":0}' >/dev/null
  sleep 1
} | ucode /tmp/uniwrt/agent/main.uc"#;
    let request = serde_json::json!({
        "addr": CLIENT, "dialog_token": 1, "validity_period": 100, "neighbors": [],
        "mbo_reason": 0, "cell_pref": 255, "reassoc_delay": 0
    });
    let out = node.sh(script, &[iface, &request.to_string()]);
    out.lines()
        .filter_map(|line| {
            let line: Value = serde_json::from_str(line).ok()?;
            let event = AgentEvent::Event {
                topic: line["ev"].as_str()?.to_owned(),
                ts: None,
                data: Arc::from(RawValue::from_string(line["data"].to_string()).ok()?),
            };
            match events::parse(&event) {
                Ok(Some(Push::Event(NodeEvent::Steer(steer)))) => Some(steer),
                _ => None,
            }
        })
        .collect()
}

/// Steering's pushes on `node`, which serves client c; c is back on it at the end.
fn steering_is_reported(node: &Node) {
    let iface = serving(node).expect("a serves client c");
    // usteer 2022.08's request, without the MBO fields: hostapd refuses it on both releases.
    assert!(
        !transition_request(node, &iface, false),
        "{}: hostapd took a transition request without the MBO fields: usteer's requests \
         reach clients now (update features/wifi.md, Steering & balancing)",
        node.container
    );
    let pushed = steered(node, &iface);
    let actions: Vec<_> = pushed.iter().map(|steer| steer.action).collect();
    assert!(
        pushed.iter().any(|steer| matches!(
            steer.action,
            SteerAction::TransitionAccepted | SteerAction::TransitionRejected
        ) && steer.mac == Some(CLIENT.parse().expect("a MAC"))),
        "{}: no transition answer: {actions:?}",
        node.container
    );
    let kick = pushed
        .iter()
        .find(|steer| steer.action == SteerAction::SignalKick)
        .unwrap_or_else(|| panic!("{}: no signal kick: {actions:?}", node.container));
    assert_eq!(
        (kick.iface.as_str(), kick.min_signal_dbm),
        (iface.as_str(), Some(-1)),
        "{}: {kick:?}",
        node.container
    );
    assert!(
        kick.signal_dbm.is_some_and(|signal| signal < -1),
        "{kick:?}"
    );
    let deadline = Instant::now() + BACK;
    while serving(node).is_none() {
        assert!(
            Instant::now() < deadline,
            "{}: client c didn't come back",
            node.container
        );
        thread::sleep(Duration::from_secs(2));
    }
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
            config["roam_trigger_snr"], 0,
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

    client_info_places_the_client(&nodes[0].0);
    steering_is_reported(&nodes[0].0);

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
