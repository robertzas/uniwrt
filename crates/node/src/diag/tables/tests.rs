//! The parsers on output captured from OpenWrt 24.10 and 25.12 (BusyBox's `ip`, a booted
//! 25.12's conntrack) and on iproute2's words (`ip-full`).

use std::net::IpAddr;

use uniwrt_model::diag::{
    IpFamily,
    tables::{NextHop, TrackedFlow},
};

use super::{parse_conntrack, parse_count, parse_neighbors, parse_routes};

fn ip(text: &str) -> IpAddr {
    text.parse().unwrap()
}

#[test]
fn neighbors_of_both_ips() {
    let busybox =
        "172.17.0.1 dev br-lan lladdr e2:a8:7a:17:91:f7 ref 1 used 0/0/0 probes 4 REACHABLE
fe80::e0a8:7aff:fe17:91f7 dev br-lan lladdr e2:a8:7a:17:91:f7 ref 1 used 0/0/0 probes 0 DELAY
fe80::9084:eeff:feaa:72a1 dev br-lan  used 0/0/0 probes 6 FAILED
";
    let rows = parse_neighbors(busybox);
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].address, ip("172.17.0.1"));
    assert_eq!(
        rows[0].mac.map(|mac| mac.to_string()).as_deref(),
        Some("e2:a8:7a:17:91:f7")
    );
    assert_eq!(rows[0].device, "br-lan");
    assert_eq!(rows[0].state.as_deref(), Some("REACHABLE"));
    assert_eq!(rows[1].state.as_deref(), Some("DELAY"));
    assert_eq!(
        (rows[2].mac, rows[2].state.as_deref()),
        (None, Some("FAILED"))
    );
    let iproute2 = "fe80::1 dev wan lladdr 00:11:22:33:44:55 router STALE
192.168.1.20 dev br-lan lladdr 00:11:22:33:44:66 proto zebra PERMANENT
192.168.1.21 dev br-lan  INCOMPLETE
";
    let rows = parse_neighbors(iproute2);
    assert!(rows[0].router);
    assert_eq!(rows[0].state.as_deref(), Some("STALE"));
    assert_eq!(rows[1].state.as_deref(), Some("PERMANENT"));
    assert_eq!(rows[2].state.as_deref(), Some("INCOMPLETE"));
    assert_eq!(parse_neighbors("").len(), 0);
}

#[test]
fn routes_of_every_table() {
    let v4 = "default via 172.17.0.1 dev br-lan 
172.17.0.0/16 dev br-lan scope link  src 172.17.0.2 
local 127.0.0.0/8 dev lo table local scope host  src 127.0.0.1 
broadcast 192.168.1.255 dev br-lan table local scope link  src 192.168.1.1 
";
    let rows = parse_routes(v4, IpFamily::Ipv4);
    assert_eq!(rows.len(), 4);
    assert_eq!(
        (
            rows[0].kind.as_str(),
            rows[0].destination.as_str(),
            rows[0].table.as_str()
        ),
        ("unicast", "default", "main")
    );
    assert_eq!(
        rows[0].hops,
        [NextHop {
            via: Some(ip("172.17.0.1")),
            device: Some("br-lan".to_owned()),
            weight: None
        }]
    );
    assert_eq!(rows[1].scope.as_deref(), Some("link"));
    assert_eq!(rows[1].source, Some(ip("172.17.0.2")));
    assert_eq!(rows[1].hops[0].via, None);
    assert_eq!(
        (
            rows[2].kind.as_str(),
            rows[2].destination.as_str(),
            rows[2].table.as_str()
        ),
        ("local", "127.0.0.0/8", "local")
    );
    assert_eq!(rows[3].kind, "broadcast");

    let v6 = "fdbe:db4c:4a1e::/64 dev br-lan  metric 1024 
unreachable fdbe:db4c:4a1e::/48 dev lo  metric 2147483647 
multicast ff00::/8 dev br-lan table local  metric 256 
default from 2001:db8::/64 via fe80::1 dev wan proto static metric 512 pref medium
default proto ra metric 1024 expires 1789sec pref medium
	nexthop via fe80::1 dev wan weight 1 
	nexthop via fe80::2 dev wan weight 2 
";
    let rows = parse_routes(v6, IpFamily::Ipv6);
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0].metric, Some(1024));
    assert_eq!(rows[1].kind, "unreachable");
    assert_eq!(rows[1].metric, Some(2_147_483_647));
    assert_eq!(rows[2].table, "local");
    assert_eq!(rows[3].protocol.as_deref(), Some("static"));
    assert_eq!(rows[3].hops[0].via, Some(ip("fe80::1")));
    assert_eq!(rows[3].family, IpFamily::Ipv6);
    let hops: Vec<_> = rows[4]
        .hops
        .iter()
        .map(|hop| (hop.via, hop.weight))
        .collect();
    assert_eq!(
        hops,
        [
            (Some(ip("fe80::1")), Some(1)),
            (Some(ip("fe80::2")), Some(2))
        ]
    );
    assert_eq!(rows[4].metric, Some(1024));
}

#[test]
fn tracked_connections() {
    let text = "ipv4     2 tcp      6 119 FIN_WAIT src=172.17.0.2 dst=151.101.130.132 sport=48450 dport=80 src=151.101.130.132 dst=172.17.0.2 sport=80 dport=48450 [ASSURED] mark=0 zone=0 use=2
ipv4     2 icmp     1 29 src=172.17.0.2 dst=1.1.1.1 type=8 code=0 id=1827 src=1.1.1.1 dst=172.17.0.2 type=0 code=0 id=1827 mark=0 zone=0 use=2
ipv6     10 icmpv6   58 29 src=fe80:0000:0000:0000:20f5:abff:fe44:766b dst=ff02:0000:0000:0000:0000:0000:0000:0001 type=128 code=0 id=1830 [UNREPLIED] src=ff02:0000:0000:0000:0000:0000:0000:0001 dst=fe80:0000:0000:0000:20f5:abff:fe44:766b type=129 code=0 id=1830 mark=0 zone=0 use=2
ipv4     2 udp      17 54 src=192.168.1.20 dst=9.9.9.9 sport=40000 dport=53 packets=1 bytes=60 src=9.9.9.9 dst=203.0.113.7 sport=53 dport=40000 packets=1 bytes=120 [OFFLOAD] mark=16 zone=0 use=2
garbage line
";
    let rows = parse_conntrack(text);
    assert_eq!(rows.len(), 4);
    let tcp = &rows[0];
    assert_eq!(
        (tcp.protocol.as_str(), tcp.protocol_number, tcp.expires_s),
        ("tcp", 6, 119)
    );
    assert_eq!(tcp.state.as_deref(), Some("FIN_WAIT"));
    assert_eq!(
        tcp.original,
        TrackedFlow {
            src: ip("172.17.0.2"),
            dst: ip("151.101.130.132"),
            sport: Some(48450),
            dport: Some(80),
            packets: None,
            bytes: None
        }
    );
    assert_eq!(tcp.reply.sport, Some(80));
    assert!(tcp.assured && !tcp.unreplied);
    assert_eq!(
        (rows[1].state.as_deref(), rows[1].original.sport),
        (None, None)
    );
    let v6 = &rows[2];
    assert_eq!(v6.family, IpFamily::Ipv6);
    assert_eq!(v6.original.dst, ip("ff02::1"));
    assert!(v6.unreplied);
    let nat = &rows[3];
    assert_eq!(
        nat.reply.dst,
        ip("203.0.113.7"),
        "the reply comes to the WAN"
    );
    assert_eq!(
        (nat.original.packets, nat.reply.bytes),
        (Some(1), Some(120))
    );
    assert!(nat.offloaded);
    assert_eq!(nat.mark, 16);
}

#[test]
fn the_count_and_its_limit() {
    assert_eq!(parse_count("5\n262144\n"), (5, Some(262_144)));
    assert_eq!(parse_count("7\n"), (7, None));
    assert_eq!(parse_count(""), (0, None));
}
