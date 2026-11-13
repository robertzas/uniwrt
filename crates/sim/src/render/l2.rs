//! The sets `leases` (dnsmasq's lease file, odhcpd's ubus leases and, where the gateway runs
//! miniupnpd, its lease file) and `neighbors` (the ARP and NDP caches and the bridge's
//! forwarding database over rtnetlink).

use std::{collections::BTreeMap, fmt::Write as _};

use serde_json::{Value, json};

use super::net::LAN_PREFIX6;
use crate::world::{
    BENCH_PORT, Client, World,
    catalog::{Kind, WAN_VLAN},
    clients::LEASE_MS,
};

/// rtnetlink's neighbor states.
const REACHABLE: u32 = 2;
const STALE: u32 = 4;

/// The leases; `files` is the node's `/etc/config`, whose `upnpd` says whether miniupnpd runs.
pub(super) fn leases(world: &World, n: usize, files: &BTreeMap<String, String>) -> Value {
    let now_s = world.now_ms() / 1000;
    if n != 0 {
        // Access points serve no DHCP.
        return json!({
            "now": now_s,
            "dnsmasq": [],
            "odhcpd": {"ipv4leases": null, "ipv6leases": null},
        });
    }
    let mut data = String::new();
    let mut line = |lease_ms: i64, mac: String, ip: String, name: Option<&str>| {
        let expiry = (lease_ms + LEASE_MS) / 1000;
        if expiry > now_s {
            let name = name.unwrap_or("*");
            let _ = writeln!(data, "{expiry} {mac} {ip} {name} 01:{mac}");
        }
    };
    for (_, node) in world
        .nodes
        .iter()
        .enumerate()
        .skip(1)
        .filter(|(m, _)| world.on_lan(*m))
    {
        line(
            node.lease_ms,
            node.mac.to_string(),
            node.ip.to_string(),
            Some(&node.name),
        );
    }
    for client in &world.clients {
        line(
            client.lease_ms,
            client.mac.to_string(),
            client.ip.to_string(),
            client.hostname.as_deref(),
        );
    }
    let leases6: Vec<_> = world
        .clients
        .iter()
        .filter(|client| client.home() && World::attachment(client).is_some())
        .filter_map(|client| {
            let ip6 = client.ip6?;
            let left = (client.lease_ms + LEASE_MS) / 1000 - now_s;
            Some(json!({
                "duid": client.duid(),
                "iaid": 1,
                "hostname": client.hostname.as_deref().unwrap_or_default(),
                "accept-reconf": false,
                "assigned": u16::from_be_bytes(ip6.octets()[14..].try_into().unwrap_or_default()),
                "flags": ["bound"],
                "ipv6-addr": [{"address": ip6.to_string(), "preferred-lifetime": left / 2, "valid-lifetime": left}],
                "valid": left,
            }))
        })
        .collect();
    json!({
        "now": now_s,
        "upnp": upnp(world, files),
        "dnsmasq": [{"file": "/tmp/dhcp.leases", "data": data}],
        "odhcpd": {
            "ipv4leases": null,
            "ipv6leases": {"device": {"br-lan": {"leases": leases6}}},
        },
    })
}

pub(super) fn neighbors(world: &World, n: usize) -> Value {
    let node = &world.nodes[n];
    let mut neigh = Vec::new();
    let mut fdb = Vec::new();
    let attached = world
        .clients
        .iter()
        .filter(|client| client.home())
        .filter_map(|client| World::attachment(client).map(|at| (client, at)));
    if node.gateway {
        let vlan = format!("{}.{WAN_VLAN}", node.model.wan_port);
        neigh.push(entry(
            2,
            &vlan,
            "203.0.113.1",
            "00:00:5e:00:01:c9",
            REACHABLE,
        ));
        for (m, other) in world.nodes.iter().enumerate().skip(1) {
            if !other.up() || !world.on_lan(m) {
                continue;
            }
            neigh.push(entry(
                2,
                "br-lan",
                &other.ip.to_string(),
                &other.mac.to_string(),
                REACHABLE,
            ));
            // The bench device is on the lent adoption port, the others on the mesh.
            let dev = if other.bench { BENCH_PORT } else { "bat0" };
            fdb.push(json!({"dev": dev, "master": "br-lan", "lladdr": other.mac.to_string()}));
        }
        for (client, at) in attached {
            let state = if recent(client) { REACHABLE } else { STALE };
            let mac = client.mac.to_string();
            neigh.push(entry(2, "br-lan", &client.ip.to_string(), &mac, state));
            if let Some(ip6) = client.ip6 {
                neigh.push(entry(10, "br-lan", &ip6.to_string(), &mac, state));
            }
            fdb.push(json!({"dev": port(client, at, n), "master": "br-lan", "lladdr": mac}));
        }
    } else {
        let gateway = &world.nodes[0];
        neigh.push(entry(
            2,
            "br-lan",
            &gateway.ip.to_string(),
            &gateway.mac.to_string(),
            REACHABLE,
        ));
        neigh.push(entry(
            10,
            "br-lan",
            &format!("{LAN_PREFIX6}1"),
            &gateway.mac.to_string(),
            STALE,
        ));
        fdb.push(json!({"dev": "bat0", "master": "br-lan", "lladdr": gateway.mac.to_string()}));
        for (client, at) in attached {
            let mac = client.mac.to_string();
            fdb.push(json!({"dev": port(client, at, n), "master": "br-lan", "lladdr": mac}));
        }
    }
    // A cable both ends bridge: each learns the other's LAN address on its port.
    for &port in node.model.lan_ports.iter().chain([&node.model.wan_port]) {
        if let Some(to) = world.bridged_peer(n, port) {
            let mac = world.nodes[to].mac.to_string();
            fdb.push(json!({"dev": port, "master": "br-lan", "lladdr": mac}));
        }
    }
    json!({"rtnl": {"neigh": neigh, "fdb": fdb}})
}

fn entry(family: u8, dev: &str, dst: &str, lladdr: &str, state: u32) -> Value {
    json!({"family": family, "dev": dev, "dst": dst, "lladdr": lladdr, "state": state, "flags": 0})
}

/// The bridge port a client is learned on at node `n`: its own, else batman's.
fn port(client: &Client, at: usize, n: usize) -> &'static str {
    if at != n {
        return "bat0";
    }
    match (&client.assoc, client.port) {
        (Some(assoc), _) => assoc.band.ap_iface(),
        (None, Some((_, port))) => port,
        (None, None) => "bat0",
    }
}

fn recent(client: &Client) -> bool {
    client.port.is_some()
        || client
            .assoc
            .as_ref()
            .is_some_and(|assoc| assoc.inactive_ms < 30_000)
}

/// miniupnpd's lease file where `upnpd` turns it on: the TVs and laptops at home keep a port
/// open (a media server, a game), for an hour from when they last came home.
fn upnp(world: &World, files: &BTreeMap<String, String>) -> Value {
    let on = files
        .get("upnpd")
        .and_then(|text| uniwrt_uci::parse("upnpd", text).ok())
        .and_then(|package| {
            package
                .section("config")?
                .option("enabled")
                .map(|e| e == "1")
        })
        .unwrap_or(false);
    if !on {
        return Value::Null;
    }
    let mut data = String::new();
    let open = world
        .clients
        .iter()
        .filter(|client| client.home() && matches!(client.spec.kind, Kind::Tv | Kind::Laptop));
    for (i, client) in open.enumerate() {
        let port = 32400 + u16::try_from(i).unwrap_or(0);
        let expiry = (client.lease_ms + LEASE_MS) / 1000;
        let name = client.hostname.as_deref().unwrap_or("client");
        let _ = writeln!(
            data,
            "TCP:{port}:{}:{port}:{expiry}:{name} media",
            client.ip
        );
    }
    json!({"file": "/var/run/miniupnpd.leases", "data": data})
}
