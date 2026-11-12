//! Rendering the world as a node's agent reports it: every collect set in the raw shapes of
//! `node-agent/collect/` ([`collect`]), the hello, hostapd's and dnsmasq's notifications, and
//! log batches in both of `log_follow`'s modes, and the `scan` op's answer. The engine parses these with the same parsers
//! as a real node's, so the simulation exercises them.

mod batman;
pub(crate) mod commands;
pub(crate) mod config;
mod devices;
mod feeds;
mod l2;
mod logs;
mod net;
pub(crate) mod scan;
mod system;
pub(crate) mod wifi;
pub(crate) mod wpad;

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value, json};
use uniwrt_node::{
    Hello,
    proto::{
        AgentCaps,
        ops::{Set, Topic},
    },
};

use crate::world::{Happening, What, World};

pub(crate) use self::logs::{log_entries, log_lines};

/// The ops a simulated agent answers.
const OPS: [&str; 9] = [
    "bss",
    "collect",
    "log_follow",
    "ping",
    "read_file",
    "scan",
    "set_interval",
    "subscribe",
    "unsubscribe",
];

pub(crate) fn hello(world: &World, node: usize) -> Hello {
    Hello {
        proto: 1,
        agent: "sim".to_owned(),
        release: Some(world.nodes[node].release.version.to_owned()),
        caps: AgentCaps {
            events: true,
            ops: OPS.iter().map(|&op| op.to_owned()).collect(),
            sets: Set::ALL.iter().map(|set| set.as_str().to_owned()).collect(),
            topics: Topic::ALL
                .iter()
                .map(|topic| topic.as_str().to_owned())
                .collect(),
        },
    }
}

/// A collect answer (or a tick's body) for `sets`; `files` is the node's `/etc/config`.
pub(crate) fn collect(
    world: &World,
    node: usize,
    sets: &[Set],
    files: &BTreeMap<String, String>,
    off: &BTreeSet<String>,
    wpad: &str,
) -> Value {
    let sets: Map<_, _> = sets
        .iter()
        .map(|&set| {
            let value = match set {
                Set::Config => config::hashes(files),
                Set::Interfaces => net::interfaces(world, node, files),
                Set::Leases => l2::leases(world, node, files),
                Set::Radios => wifi::radios(world, node, &config::aps(files), off),
                Set::Stations => wifi::stations(world, node, off),
                Set::Capabilities => system::capabilities(&world.nodes[node], wpad),
                _ => set_of(world, node, set),
            };
            (set.as_str().to_owned(), value)
        })
        .collect();
    json!({"sets": sets, "errors": {}})
}

fn set_of(world: &World, n: usize, set: Set) -> Value {
    let node = &world.nodes[n];
    match set {
        Set::Batman => batman::batman(world, n),
        Set::Board => system::board(node),
        Set::Capabilities => system::capabilities(node, wpad::MESH),
        Set::Config => json!({"files": {}}),
        Set::Devices => devices::devices(world, n),
        Set::Interfaces => net::interfaces(world, n, &BTreeMap::new()),
        Set::Leases => l2::leases(world, n, &BTreeMap::new()),
        Set::Neighbors => l2::neighbors(world, n),
        Set::Phys => wifi::phys(),
        Set::Radios => wifi::radios(world, n, &[], &BTreeSet::new()),
        Set::Stations => wifi::stations(world, n, &BTreeSet::new()),
        Set::System => system::system(world, node),
    }
}

/// A happening as the agent pushes it: its topic, the topic's group and the data; `None` for
/// what is no notification (a node going down or coming up).
pub(crate) fn notification(world: &World, happening: &Happening) -> Option<(Topic, String, Value)> {
    let node = &world.nodes[happening.node];
    let hostapd = |kind: &str, data: Value| Some((Topic::Hostapd, format!("hostapd.{kind}"), data));
    match &happening.what {
        What::Auth { mac, band, signal } | What::Assoc { mac, band, signal } => {
            let kind = if matches!(happening.what, What::Auth { .. }) {
                "auth"
            } else {
                "assoc"
            };
            let radio = &node.radios[band.index()];
            hostapd(
                kind,
                json!({
                    "address": mac.to_string(),
                    "ifname": band.ap_iface(),
                    "target": node.bssid(*band).to_string(),
                    "signal": signal,
                    "freq": band.freq(radio.channel),
                }),
            )
        }
        What::Authorized { mac, band, alg } => hostapd(
            "sta-authorized",
            json!({"address": mac.to_string(), "ifname": band.ap_iface(), "auth-alg": alg}),
        ),
        What::Disassoc { mac, band } => hostapd(
            "disassoc",
            json!({"address": mac.to_string(), "ifname": band.ap_iface()}),
        ),
        // ucode reads hostapd's u8 status as a boolean: `false` is 0, accepted.
        What::Steered { mac, band, target } => hostapd(
            "bss-transition-response",
            json!({
                "address": mac.to_string(),
                "ifname": band.ap_iface(),
                "status-code": false,
                "target-bssid": target.to_string(),
            }),
        ),
        What::Lease { mac, ip, name } => Some((
            Topic::Dhcp,
            "dnsmasq.dhcp.ack".to_owned(),
            json!({"mac": mac.to_string(), "ip": ip.to_string(), "name": name.as_deref().unwrap_or_default(), "interface": "br-lan"}),
        )),
        What::Down | What::Up => None,
    }
}

#[cfg(test)]
mod cable_tests;
#[cfg(test)]
mod tests;
