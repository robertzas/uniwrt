//! A simulated node's neighbor, route and connection tables (D-142), in the words of OpenWrt's
//! BusyBox `ip` and `/proc/net/nf_conntrack` (captured on 24.10 and 25.12), for the commands
//! of `uniwrt_node::diag::tables`: the gateway sees every client on the LAN, the ISP's router
//! and the nodes; an access point the gateway and its own clients. The gateway tracks a few
//! connections per client, translated to its WAN lease; an access point only the controller's SSH.

use std::{collections::BTreeMap, fmt::Write as _, net::Ipv4Addr};

use uniwrt_node::{ExecOutput, diag::tables};

use super::commands::ok;
use crate::{isp, world::World};

/// The connections a simulated node can track.
const CONNTRACK_MAX: u32 = 16_384;

/// Answers the table commands on node `n`; `None` for others.
pub(crate) fn exec(
    world: &World,
    n: usize,
    files: &BTreeMap<String, String>,
    argv: &[String],
) -> Option<ExecOutput> {
    let argv: Vec<&str> = argv.iter().map(String::as_str).collect();
    let wan = wan_device(world, n, files);
    let text = match argv.as_slice() {
        a if a == tables::NEIGHBORS => neighbors(world, n, wan.as_deref()),
        ["ip", "-4", "route", "show", "table", "all"] => routes_v4(world, n, wan.as_deref()),
        ["ip", "-6", "route", "show", "table", "all"] => routes_v6(),
        ["head", "-n", count, tables::CONNTRACK] => {
            let count: usize = count.parse().ok()?;
            conntrack(world, n)
                .into_iter()
                .take(count)
                .map(|line| line + "\n")
                .collect()
        }
        a if a == tables::CONNTRACK_COUNT => {
            format!("{}\n{CONNTRACK_MAX}\n", conntrack(world, n).len())
        }
        _ => return None,
    };
    Some(ok(text.into_bytes()))
}

/// The gateway's WAN device as its `network` has it; none on access points.
fn wan_device(world: &World, n: usize, files: &BTreeMap<String, String>) -> Option<String> {
    let node = &world.nodes[n];
    if !node.gateway {
        return None;
    }
    isp::uplink(
        files.get("network").map(String::as_str),
        node.model.wan_port,
    )
    .wan
    .map(|(_, device, _)| device)
}

/// The LAN's prefix length.
const fn lan_bits(world: &World) -> u8 {
    if world.wide { 16 } else { 24 }
}

fn network(ip: Ipv4Addr, bits: u8) -> Ipv4Addr {
    Ipv4Addr::from(u32::from(ip) & (u32::MAX << (32 - bits)))
}

/// The clients on the LAN with what reaches them through node `n`: on the gateway every one,
/// on an access point those on it.
fn clients(world: &World, n: usize) -> impl Iterator<Item = &crate::world::Client> {
    let gateway = world.nodes[n].gateway;
    world.clients.iter().filter(move |client| {
        client.away_until.is_none()
            && (gateway
                || client.assoc.as_ref().is_some_and(|assoc| assoc.node == n)
                || client.port.is_some_and(|(node, _)| node == n))
    })
}

fn neighbors(world: &World, n: usize, wan: Option<&str>) -> String {
    let mut text = String::new();
    let mut line = |ip: &dyn std::fmt::Display, mac: &dyn std::fmt::Display, dev: &str, state| {
        let _ = writeln!(
            text,
            "{ip} dev {dev} lladdr {mac} ref 1 used 0/0/0 probes 1 {state}"
        );
    };
    let gateway = &world.nodes[0];
    if let Some(wan) = wan {
        line(&isp::ROUTER, &"02:00:5e:10:00:01", wan, "REACHABLE");
        for (i, node) in world.nodes.iter().enumerate().skip(1) {
            if world.on_lan(i) && node.up() && !node.bench {
                line(&node.ip, &node.mac, "br-lan", "REACHABLE");
            }
        }
    } else if n != 0 {
        line(&gateway.ip, &gateway.mac, "br-lan", "REACHABLE");
    }
    for (i, client) in clients(world, n).enumerate() {
        let state = if i % 3 == 2 { "STALE" } else { "REACHABLE" };
        line(&client.ip, &client.mac, "br-lan", state);
        if let Some(ip6) = client.ip6 {
            line(&ip6, &client.mac, "br-lan", "STALE");
        }
    }
    text
}

fn routes_v4(world: &World, n: usize, wan: Option<&str>) -> String {
    let node = &world.nodes[n];
    let bits = lan_bits(world);
    let lan = network(node.ip, bits);
    let ip = node.ip;
    let mut text = String::new();
    match wan {
        Some(wan) => {
            let _ = writeln!(text, "default via {} dev {wan} ", isp::ROUTER);
            let _ = writeln!(
                text,
                "203.0.113.0/24 dev {wan} scope link  src {} ",
                isp::LEASE
            );
        }
        None => {
            let _ = writeln!(text, "default via {} dev br-lan ", world.nodes[0].ip);
        }
    }
    let _ = writeln!(text, "{lan}/{bits} dev br-lan scope link  src {ip} ");
    let broadcast = Ipv4Addr::from(u32::from(lan) | (u32::MAX >> bits));
    let _ = write!(
        text,
        "local 127.0.0.0/8 dev lo table local scope host  src 127.0.0.1 \n\
         local 127.0.0.1 dev lo table local scope host  src 127.0.0.1 \n\
         broadcast 127.255.255.255 dev lo table local scope link  src 127.0.0.1 \n\
         local {ip} dev br-lan table local scope host  src {ip} \n\
         broadcast {broadcast} dev br-lan table local scope link  src {ip} \n"
    );
    text
}

fn routes_v6() -> String {
    "fd00:5eed::/64 dev br-lan  metric 1024 \n\
     unreachable fd00:5eed::/48 dev lo  metric 2147483647 \n\
     fe80::/64 dev br-lan  metric 256 \n\
     local ::1 dev lo table local  metric 0 \n\
     multicast ff00::/8 dev br-lan table local  metric 256 \n"
        .to_owned()
}

/// The tracked connections, one line each.
fn conntrack(world: &World, n: usize) -> Vec<String> {
    let node = &world.nodes[n];
    let [a, b, c, _] = node.ip.octets();
    // The controller logs in from host .50 of the LAN (as `env` answers).
    let controller = Ipv4Addr::new(a, b, c, 50);
    let mut lines = vec![format!(
        "ipv4     2 tcp      6 7440 ESTABLISHED src={controller} dst={ip} sport=50022 dport=22 \
         packets=812 bytes=96044 src={ip} dst={controller} sport=22 dport=50022 packets=640 \
         bytes=188210 [ASSURED] mark=0 zone=0 use=1",
        ip = node.ip
    )];
    if !node.gateway {
        return lines;
    }
    let wan = isp::LEASE;
    for client in clients(world, n) {
        let seed = client
            .mac
            .octets()
            .iter()
            .fold(7_u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(*b)));
        let server = Ipv4Addr::new(151, 101, (seed % 200) as u8 + 1, (seed / 7 % 250) as u8 + 1);
        let port = 40_000 + (seed % 20_000) as u16;
        let packets = u64::from(seed % 900) + 20;
        lines.push(format!(
            "ipv4     2 tcp      6 {expires} ESTABLISHED src={ip} dst={server} sport={port} dport=443 \
             packets={packets} bytes={sent} src={server} dst={wan} sport=443 dport={port} \
             packets={back} bytes={received} [ASSURED] mark=0 zone=0 use=2",
            expires = 3_000 + seed % 4_000,
            ip = client.ip,
            sent = packets * 90,
            back = packets * 2,
            received = packets * 1_300,
        ));
        lines.push(format!(
            "ipv4     2 udp      17 {expires} src={ip} dst={gw} sport={dns} dport=53 packets=1 bytes=71 \
             src={gw} dst={ip} sport=53 dport={dns} packets=1 bytes=103 mark=0 zone=0 use=2",
            expires = seed % 60,
            ip = client.ip,
            gw = node.ip,
            dns = port + 1,
        ));
    }
    lines
}

#[cfg(test)]
mod tests {
    use uniwrt_model::diag::IpFamily;
    use uniwrt_node::diag::tables;

    use crate::{SimConfig, world::World};

    #[test]
    fn the_tables_parse_as_openwrt_s() {
        let world = World::new(
            &SimConfig::default(),
            uniwrt_core::Timestamp::from_unix_ms(1_793_245_539_000),
        );
        let files = std::collections::BTreeMap::new();
        let run = |n: usize, argv: &[&str]| {
            let argv: Vec<String> = argv.iter().map(|s| (*s).to_owned()).collect();
            let output = super::exec(&world, n, &files, &argv).expect("a table command");
            String::from_utf8(output.stdout).unwrap()
        };
        let neighbors = tables::parse_neighbors(&run(0, &tables::NEIGHBORS));
        assert!(
            neighbors
                .iter()
                .any(|row| row.address.to_string() == "203.0.113.1")
        );
        assert!(
            neighbors.len() > 3,
            "the nodes and the clients: {neighbors:?}"
        );
        let ap = tables::parse_neighbors(&run(1, &tables::NEIGHBORS));
        assert_eq!(ap[0].address, std::net::IpAddr::V4(world.nodes[0].ip));
        let routes = tables::parse_routes(&run(0, &tables::routes(IpFamily::Ipv4)), IpFamily::Ipv4);
        assert_eq!(routes[0].destination, "default");
        assert_eq!(
            routes[0].hops[0].via.map(|ip| ip.to_string()).as_deref(),
            Some("203.0.113.1")
        );
        let v6 = tables::parse_routes(&run(0, &tables::routes(IpFamily::Ipv6)), IpFamily::Ipv6);
        assert_eq!(v6[1].kind, "unreachable");
        let argv = tables::conntrack();
        let argv: Vec<&str> = argv.iter().map(String::as_str).collect();
        let text = run(0, &argv);
        let connections = tables::parse_conntrack(&text);
        assert_eq!(
            connections.len(),
            text.lines().count(),
            "every line parses: {text}"
        );
        assert!(connections.len() > 1, "the clients' too");
        assert!(connections.iter().all(|c| c.original.src.is_ipv4()));
        let (count, max) = tables::parse_count(&run(0, &tables::CONNTRACK_COUNT));
        assert_eq!(count as usize, connections.len());
        assert_eq!(max, Some(16_384));
    }
}
