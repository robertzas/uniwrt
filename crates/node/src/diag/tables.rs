//! The neighbor, route and connection tables (D-142): the commands that read them, one exec
//! each with no shell, and their parsers. OpenWrt's `ip` is BusyBox's unless `ip-full` is
//! installed; both print the same words (BusyBox adds `ref`, `used` and `probes`, iproute2
//! `proto`, `pref` and multipath hops on lines of their own), and neither prints JSON in
//! BusyBox's build, so the text is parsed key by key and unknown words are skipped. Outputs
//! were captured on OpenWrt 24.10 and 25.12.

use std::net::IpAddr;

use uniwrt_core::MacAddr;
use uniwrt_model::diag::{
    IpFamily,
    tables::{MAX_CONNECTIONS, NeighborEntry, NextHop, Route, TrackedConnection, TrackedFlow},
};

/// Both families' neighbor caches.
pub const NEIGHBORS: [&str; 3] = ["ip", "neigh", "show"];

/// Every routing table of `family`.
pub const fn routes(family: IpFamily) -> [&'static str; 6] {
    let flag = match family {
        IpFamily::Ipv4 => "-4",
        IpFamily::Ipv6 => "-6",
    };
    ["ip", flag, "route", "show", "table", "all"]
}

/// The first [`MAX_CONNECTIONS`] tracked connections.
pub fn conntrack() -> Vec<String> {
    vec![
        "head".to_owned(),
        "-n".to_owned(),
        MAX_CONNECTIONS.to_string(),
        CONNTRACK.to_owned(),
    ]
}

/// The kernel's connection table.
pub const CONNTRACK: &str = "/proc/net/nf_conntrack";

/// How many connections are tracked, and how many can be ([`parse_count`]).
pub const CONNTRACK_COUNT: [&str; 3] = [
    "cat",
    "/proc/sys/net/netfilter/nf_conntrack_count",
    "/proc/sys/net/netfilter/nf_conntrack_max",
];

/// The neighbor cache in `ip neigh show`'s words.
pub fn parse_neighbors(text: &str) -> Vec<NeighborEntry> {
    text.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let address: IpAddr = words.next()?.parse().ok()?;
            let mut neighbor = NeighborEntry {
                address,
                mac: None,
                device: String::new(),
                state: None,
                router: false,
            };
            let mut states: Vec<&str> = Vec::new();
            while let Some(word) = words.next() {
                match word {
                    "dev" => words.next()?.clone_into(&mut neighbor.device),
                    "lladdr" => neighbor.mac = words.next()?.parse::<MacAddr>().ok(),
                    "router" => neighbor.router = true,
                    "proto" | "used" | "probes" | "ref" | "vrf" | "nud" => {
                        words.next();
                    }
                    state if is_upper_word(state) => states.push(state),
                    _ => {}
                }
            }
            neighbor.state = (!states.is_empty()).then(|| states.join(" "));
            Some(neighbor)
        })
        .collect()
}

/// Whether `word` is a state such as `REACHABLE` or `NOARP`.
fn is_upper_word(word: &str) -> bool {
    !word.is_empty() && word.bytes().all(|b| b.is_ascii_uppercase() || b == b'_')
}

/// The route kinds `ip` prints before a destination; a route without one is `unicast`.
const KINDS: [&str; 10] = [
    "unicast",
    "local",
    "broadcast",
    "multicast",
    "anycast",
    "unreachable",
    "blackhole",
    "prohibit",
    "throw",
    "nat",
];

/// Words of a route that `ip` follows with a value this parser doesn't keep.
const SKIPPED: [&str; 18] = [
    "pref", "expires", "mtu", "advmss", "hoplimit", "rtt", "rttvar", "cwnd", "initcwnd",
    "initrwnd", "realms", "realm", "from", "tos", "dsfield", "nhid", "ssthresh", "features",
];

/// The routes of `family` in `ip route show table all`'s words; a multipath route's hops
/// follow on lines of their own, starting with `nexthop`.
pub fn parse_routes(text: &str, family: IpFamily) -> Vec<Route> {
    let mut routes: Vec<Route> = Vec::new();
    for line in text.lines() {
        let continued = line.starts_with(char::is_whitespace);
        let mut words = line.split_whitespace().peekable();
        if continued {
            if let Some(route) = routes.last_mut() {
                hop_words(route, &mut words);
            }
            continue;
        }
        let Some(first) = words.next() else {
            continue;
        };
        let (kind, destination) = if KINDS.contains(&first) {
            match words.next() {
                Some(destination) => (first, destination),
                None => continue,
            }
        } else {
            ("unicast", first)
        };
        let mut route = Route {
            family,
            kind: kind.to_owned(),
            destination: destination.to_owned(),
            table: "main".to_owned(),
            protocol: None,
            scope: None,
            source: None,
            metric: None,
            hops: Vec::new(),
        };
        hop_words(&mut route, &mut words);
        routes.push(route);
    }
    routes
}

/// Reads a route's words into it, and those of its hops into its last hop: `via`, `dev` and
/// `weight` start a hop when none is open, and `nexthop` always does.
fn hop_words<'a>(route: &mut Route, words: &mut impl Iterator<Item = &'a str>) {
    let mut open = false;
    let hop = |route: &mut Route, open: &mut bool| -> usize {
        if !*open {
            route.hops.push(NextHop {
                via: None,
                device: None,
                weight: None,
            });
            *open = true;
        }
        route.hops.len() - 1
    };
    while let Some(word) = words.next() {
        match word {
            "nexthop" => open = false,
            "via" => {
                let mut value = words.next();
                // iproute2 names the family of a gateway of another (`via inet6 fe80::1`).
                if matches!(value, Some("inet" | "inet6")) {
                    value = words.next();
                }
                let i = hop(route, &mut open);
                route.hops[i].via = value.and_then(|v| v.parse().ok());
            }
            "dev" => {
                let i = hop(route, &mut open);
                route.hops[i].device = words.next().map(str::to_owned);
            }
            "weight" => {
                let i = hop(route, &mut open);
                route.hops[i].weight = words.next().and_then(|v| v.parse().ok());
            }
            "table" => {
                if let Some(table) = words.next() {
                    table.clone_into(&mut route.table);
                }
            }
            "proto" => route.protocol = words.next().map(str::to_owned),
            "scope" => route.scope = words.next().map(str::to_owned),
            "src" => route.source = words.next().and_then(|v| v.parse().ok()),
            "metric" => route.metric = words.next().and_then(|v| v.parse().ok()),
            skipped if SKIPPED.contains(&skipped) => {
                words.next();
            }
            _ => {}
        }
    }
}

/// The connections in `/proc/net/nf_conntrack`: the family and protocol by name and number,
/// the seconds left, TCP's state, then each direction's `key=value` words (the second `src=`
/// starts the reply) and the flags in brackets.
pub fn parse_conntrack(text: &str) -> Vec<TrackedConnection> {
    text.lines().filter_map(connection).collect()
}

fn connection(line: &str) -> Option<TrackedConnection> {
    let mut words = line.split_whitespace().peekable();
    let family = match words.next()? {
        "ipv4" => IpFamily::Ipv4,
        "ipv6" => IpFamily::Ipv6,
        _ => return None,
    };
    words.next()?;
    let protocol = words.next()?.to_owned();
    let protocol_number = words.next()?.parse().ok()?;
    let expires_s = words.next()?.parse().ok()?;
    let state = words.next_if(|word| is_upper_word(word)).map(str::to_owned);
    let (mut original, mut reply) = (Pairs::default(), Pairs::default());
    let mut replying = false;
    let (mut assured, mut unreplied, mut offloaded, mut mark) = (false, false, false, 0);
    for word in words {
        match word {
            "[ASSURED]" => assured = true,
            "[UNREPLIED]" => unreplied = true,
            "[OFFLOAD]" | "[HW_OFFLOAD]" => offloaded = true,
            _ => {
                let Some((key, value)) = word.split_once('=') else {
                    continue;
                };
                if key == "src" && original.src.is_some() {
                    replying = true;
                }
                let side = if replying { &mut reply } else { &mut original };
                match key {
                    "src" => side.src = value.parse().ok(),
                    "dst" => side.dst = value.parse().ok(),
                    "sport" => side.sport = value.parse().ok(),
                    "dport" => side.dport = value.parse().ok(),
                    "packets" => side.packets = value.parse().ok(),
                    "bytes" => side.bytes = value.parse().ok(),
                    "mark" => mark = value.parse().unwrap_or(0),
                    _ => {}
                }
            }
        }
    }
    Some(TrackedConnection {
        family,
        protocol,
        protocol_number,
        state,
        expires_s,
        original: original.flow()?,
        reply: reply.flow()?,
        assured,
        unreplied,
        offloaded,
        mark,
    })
}

/// One direction's words as they come.
#[derive(Default)]
struct Pairs {
    src: Option<IpAddr>,
    dst: Option<IpAddr>,
    sport: Option<u16>,
    dport: Option<u16>,
    packets: Option<u64>,
    bytes: Option<u64>,
}

impl Pairs {
    fn flow(self) -> Option<TrackedFlow> {
        Some(TrackedFlow {
            src: self.src?,
            dst: self.dst?,
            sport: self.sport,
            dport: self.dport,
            packets: self.packets,
            bytes: self.bytes,
        })
    }
}

/// [`CONNTRACK_COUNT`]'s output: the count, then the most (absent where the file is).
pub fn parse_count(text: &str) -> (u32, Option<u32>) {
    let mut numbers = text.lines().map(|line| line.trim().parse::<u32>().ok());
    let count = numbers.next().flatten().unwrap_or(0);
    (count, numbers.next().flatten())
}

#[cfg(test)]
mod tests;
