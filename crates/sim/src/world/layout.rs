//! Building the world from a [`SimConfig`]: the gateway and the access points at their places,
//! then the clients dealt from the deck, settled where they would be after a while.

use std::{
    collections::BTreeMap,
    net::{Ipv4Addr, Ipv6Addr},
};

use uniwrt_core::MacAddr;

use super::{
    Client, Counters, Logd, Node, Radio, World,
    catalog::{
        AP_MODEL, DECK, GATEWAY_BACKHAUL_PORT, GATEWAY_MODEL, KINDS, KindSpec, NODE_OUI,
        NODE_PLACES, RELEASES, SPOTS, kind_spec,
    },
    radio::Band,
};
use crate::{SimConfig, rng::Rng};

const DAY_MS: f64 = 86_400_000.0;

pub(super) fn build(config: &SimConfig, epoch_ms: i64) -> World {
    let mut rng = Rng::new(config.seed);
    let base = 100.max(config.access_points + 2);
    let wide = base + config.clients > 254;
    let nodes: Vec<_> = (0..=config.access_points)
        .map(|index| node(&mut rng, index, config, epoch_ms, wide))
        .collect();
    let mut dealt = [0_usize; KINDS.len()];
    let mut clients = Vec::with_capacity(config.clients);
    for index in 0..config.clients {
        let spec = kind_spec(DECK[index % DECK.len()]);
        let kind = KINDS
            .iter()
            .position(|other| other.kind == spec.kind)
            .unwrap_or(0);
        let nth = dealt[kind];
        dealt[kind] += 1;
        let ip = lan_ip(base + index, wide);
        clients.push(client(&mut rng, &nodes, spec, nth, ip, epoch_ms));
    }
    let reboot_mean = config.reboot_every.map(|every| every.as_secs_f64());
    let next_reboot = reboot_mean.map_or(u64::MAX, |mean| {
        super::WARMUP_S + 120 + rng.exp(mean) as u64
    });
    let mut world = World {
        rng,
        epoch_ms,
        now_s: 0,
        nodes,
        clients,
        reboot_mean,
        next_reboot,
    };
    world.settle();
    world
}

/// A host of the LAN: 192.168.1.0/24 while the fleet fits, else 192.168.0.0/16.
pub(crate) fn lan_ip(host: usize, wide: bool) -> Ipv4Addr {
    if wide {
        Ipv4Addr::new(192, 168, (host / 256) as u8, (host % 256) as u8)
    } else {
        Ipv4Addr::new(192, 168, 1, host as u8)
    }
}

fn node(rng: &mut Rng, index: usize, config: &SimConfig, epoch_ms: i64, wide: bool) -> Node {
    let gateway = index == 0;
    let (name, pos) = NODE_PLACES.get(index).map_or_else(
        || {
            let extra = index - NODE_PLACES.len();
            (
                format!("ap-{index}"),
                (
                    6.0 + 9.0 * (extra % 3) as f64,
                    24.0 + 9.0 * (extra / 3) as f64,
                ),
            )
        },
        |(name, pos)| ((*name).to_owned(), *pos),
    );
    let model = if gateway { &GATEWAY_MODEL } else { &AP_MODEL };
    // The last access point still runs the older release, as in a fleet mid-upgrade.
    let release = &RELEASES[usize::from(index > 0 && index == config.access_points)];
    let mac = MacAddr::new([
        NODE_OUI[0],
        NODE_OUI[1],
        NODE_OUI[2],
        (index >> 8) as u8,
        index as u8,
        0x10,
    ]);
    let uptime_days = if gateway {
        rng.range(12.0, 40.0)
    } else {
        rng.range(1.0, 20.0)
    };
    let booted_ms = epoch_ms - (uptime_days * DAY_MS) as i64;
    let uptime_s = uptime_days * 86_400.0;
    let mut ports = BTreeMap::new();
    for port in model.lan_ports {
        ports.insert((*port).to_owned(), Counters::default());
    }
    ports.insert(model.wan_port.to_owned(), Counters::default());
    if gateway {
        ports.insert(GATEWAY_BACKHAUL_PORT.to_owned(), Counters::default());
        // What the uplink carried before the simulation started: about 3 Mbit/s down.
        let mut wan = Counters::default();
        wan.add(
            (uptime_s * rng.range(300e3, 450e3)) as u64,
            (uptime_s * rng.range(30e3, 60e3)) as u64,
        );
        ports.insert(model.wan_port.to_owned(), wan);
    }
    let radio = |band: Band, channel: u8, txpower: u8, noise: f64| Radio {
        band,
        channel,
        txpower,
        noise,
        busy: 5.0,
        ap: Counters::default(),
        mesh: Counters::default(),
    };
    Node {
        name,
        gateway,
        model,
        release,
        mac,
        ip: lan_ip(1 + index, wide),
        pos,
        wired: index <= 1 && config.access_points > 0,
        radios: [
            radio(Band::G2, [1, 6, 11][index % 3], 20, -90.0),
            radio(Band::G5, 36, 23, -95.0),
        ],
        booted_ms,
        down_until: None,
        lease_ms: epoch_ms - rng.range(0.0, DAY_MS / 4.0) as i64,
        load: if gateway { 0.25 } else { 0.1 },
        cpu: if gateway { 6.0 } else { 3.0 },
        memory_used: if gateway { 0.24 } else { 0.36 },
        temp: if gateway { 54.0 } else { 47.0 },
        ports,
        logd: Logd::starting_at(500 + rng.below(4000)),
    }
}

fn client(
    rng: &mut Rng,
    nodes: &[Node],
    spec: &'static KindSpec,
    nth: usize,
    ip: Ipv4Addr,
    epoch_ms: i64,
) -> Client {
    let mut octets = [0; 6];
    match spec.oui {
        Some(oui) => octets[..3].copy_from_slice(&oui),
        // Randomized: locally administered, unicast.
        None => octets[..3].copy_from_slice(&[(rng.byte() | 0x02) & 0xfe, rng.byte(), rng.byte()]),
    }
    for octet in &mut octets[3..] {
        *octet = rng.byte();
    }
    let hostname = match spec.hostnames {
        [] => None,
        names => Some(names.get(nth).map_or_else(
            || format!("{}-{}", names[nth % names.len()], nth / names.len() + 1),
            |name| (*name).to_owned(),
        )),
    };
    let spot = SPOTS[spec.spots[nth % spec.spots.len()]];
    let pos = (spot.0 + rng.range(-1.5, 1.5), spot.1 + rng.range(-1.5, 1.5));
    let port = spec.wired.then(|| {
        let nearest = (0..nodes.len())
            .min_by(|&a, &b| {
                super::distance(nodes[a].pos, pos).total_cmp(&super::distance(nodes[b].pos, pos))
            })
            .unwrap_or(0);
        let ports = nodes[nearest].model.lan_ports;
        (nearest, ports[nth % ports.len()])
    });
    let [_, _, high, low] = ip.octets();
    Client {
        mac: MacAddr::new(octets),
        hostname,
        spec,
        ip,
        ip6: spec.ipv6.then(|| {
            Ipv6Addr::new(
                0x2001,
                0xdb8,
                0x4a,
                0x100,
                0,
                0,
                0,
                u16::from_be_bytes([high, low]),
            )
        }),
        pos,
        target: None,
        next_move: spec.moves.map_or(u64::MAX, |mean| rng.exp(mean) as u64),
        away_until: None,
        next_leave: if spec.leaves {
            rng.exp(5_400.0) as u64
        } else {
            u64::MAX
        },
        fading: nodes
            .iter()
            .map(|_| [rng.normal() * 2.0, rng.normal() * 2.0])
            .collect(),
        assoc: None,
        port,
        lease_ms: epoch_ms - rng.range(0.0, DAY_MS / 4.0) as i64,
        busy: false,
        hold_until: 0,
        farewell: None,
    }
}
