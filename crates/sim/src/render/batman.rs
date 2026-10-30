//! The `batman` set: batctl's JSON for bat0 (mesh info, hard interfaces, originators,
//! neighbors, gateways, translation tables). Routes follow BATMAN IV: a link's TQ comes from
//! its signal (a wired link is perfect), and every hop costs the hop penalty.

use serde_json::{Value, json};

use crate::world::{World, catalog::GATEWAY_BACKHAUL_PORT, radio::tq};

const HOP_PENALTY: u32 = 30;
const MESH_IF: &str = "phy1-mesh0";

/// A direct link from one node to another: the interface it leaves by, the neighbor's
/// address on it, and its TQ.
struct Link {
    to: usize,
    ifname: &'static str,
    ifindex: u32,
    neigh: String,
    tq: u32,
}

fn links(world: &World, from: usize) -> Vec<Link> {
    let nodes = &world.nodes;
    let mut links = Vec::new();
    if !nodes[from].up() {
        return links;
    }
    for to in (0..nodes.len()).filter(|&to| to != from && nodes[to].up()) {
        // Slow swings of a few dB, alike in both directions.
        let phase = (world.now_s as f64 / 47.0) + (from * to) as f64;
        let signal = world.mesh_signal(from, to) + 2.0 * phase.sin();
        let tq = u32::from(tq(signal));
        if tq > 0 {
            links.push(Link {
                to,
                ifname: MESH_IF,
                ifindex: 12,
                neigh: nodes[to].mesh_mac().to_string(),
                tq,
            });
        }
        let wired = nodes[from].wired && nodes[to].wired && (from == 0 || to == 0);
        if wired {
            let ifname = if from == 0 {
                GATEWAY_BACKHAUL_PORT
            } else {
                nodes[from].model.wan_port
            };
            let neigh = nodes[to].wired_mac();
            links.push(Link {
                to,
                ifname,
                ifindex: 7,
                neigh: neigh.to_string(),
                tq: 255,
            });
        }
    }
    links
}

/// The best TQ from each node to each other one, over any number of hops.
fn best(world: &World) -> Vec<Vec<u32>> {
    let count = world.nodes.len();
    let all: Vec<_> = (0..count).map(|from| links(world, from)).collect();
    let mut best = vec![vec![0_u32; count]; count];
    for (from, links) in all.iter().enumerate() {
        best[from][from] = 255;
        for link in links {
            best[from][link.to] = best[from][link.to].max(link.tq);
        }
    }
    // Relaxes until nothing improves: as many rounds as the mesh has hops.
    for _ in 0..count {
        let mut improved = false;
        for (from, links) in all.iter().enumerate() {
            for link in links {
                let via: Vec<_> = best[link.to]
                    .iter()
                    .map(|&rest| forwarded(link.tq, rest))
                    .collect();
                for (target, via) in via.into_iter().enumerate() {
                    if target != from && via > best[from][target] {
                        best[from][target] = via;
                        improved = true;
                    }
                }
            }
        }
        if !improved {
            break;
        }
    }
    best
}

fn forwarded(link: u32, rest: u32) -> u32 {
    link * rest / 255 * (255 - HOP_PENALTY) / 255
}

/// Milliseconds since a node's originator messages were last seen: a few hundred, varying.
fn last_seen(world: &World, seed: usize) -> u64 {
    80 + ((world.now_s as usize * 7 + seed * 131) % 900) as u64
}

/// Every route to every other node, the best of each marked.
fn originators(world: &World, n: usize, links: &[Link], best: &[Vec<u32>]) -> Vec<Value> {
    let mut originators = Vec::new();
    for target in (0..world.nodes.len()).filter(|&t| t != n && world.nodes[t].up()) {
        let routes: Vec<_> = links
            .iter()
            .map(|link| {
                let tq = if link.to == target {
                    link.tq
                } else {
                    forwarded(link.tq, best[link.to][target])
                };
                (link, tq)
            })
            .filter(|(_, tq)| *tq > 0)
            .collect();
        let top = routes.iter().map(|(_, tq)| *tq).max().unwrap_or(0);
        let mut marked = false;
        for (link, tq) in routes {
            let is_best = !marked && tq == top;
            marked |= is_best;
            originators.push(json!({
                "hard_ifindex": link.ifindex,
                "hard_ifname": link.ifname,
                "orig_address": world.nodes[target].mesh_mac().to_string(),
                "best": is_best,
                "last_seen_msecs": last_seen(world, target),
                "neigh_address": link.neigh,
                "tq": tq,
            }));
        }
    }
    originators
}

/// An access point's way to the gateway: batman's gateway list.
fn gateways(world: &World, n: usize, links: &[Link], best: &[Vec<u32>]) -> Vec<Value> {
    let node = &world.nodes[n];
    let mut gateways = Vec::new();
    if !node.gateway && world.nodes[0].up() {
        let route = links
            .iter()
            .map(|link| {
                let tq = if link.to == 0 {
                    link.tq
                } else {
                    forwarded(link.tq, best[link.to][0])
                };
                (link, tq)
            })
            .max_by_key(|(_, tq)| *tq);
        if let Some((link, tq)) = route.filter(|(_, tq)| *tq > 0) {
            gateways.push(json!({
                "orig_address": world.nodes[0].mesh_mac().to_string(),
                "router": link.neigh,
                "hard_ifname": link.ifname,
                "tq": tq,
                "bandwidth_down": 10_000,
                "bandwidth_up": 2_000,
                "best": true,
            }));
        }
    }
    gateways
}

/// The clients this node bridges into the mesh (local) and those the others do (global).
fn translations(world: &World, n: usize) -> (Vec<Value>, Vec<Value>) {
    let node = &world.nodes[n];
    let (mut local, mut global) = (Vec::new(), Vec::new());
    local.push(translation(
        &node.mac.to_string(),
        None,
        false,
        true,
        Some(0),
    ));
    let others = world
        .nodes
        .iter()
        .enumerate()
        .filter(|&(at, other)| at != n && other.up());
    for (_, other) in others {
        let orig = other.mesh_mac().to_string();
        global.push(translation(
            &other.mac.to_string(),
            Some(&orig),
            false,
            false,
            None,
        ));
    }
    for client in world.clients.iter().filter(|client| client.home()) {
        let Some(at) = World::attachment(client) else {
            continue;
        };
        let wifi = client.assoc.is_some();
        let mac = client.mac.to_string();
        if at == n {
            local.push(translation(
                &mac,
                None,
                wifi,
                false,
                Some(last_seen(world, at)),
            ));
        } else if world.nodes[at].up() {
            let orig = world.nodes[at].mesh_mac().to_string();
            global.push(translation(&mac, Some(&orig), wifi, false, None));
        }
    }
    (local, global)
}

pub(super) fn batman(world: &World, n: usize) -> Value {
    let node = &world.nodes[n];
    let best = best(world);
    let links = links(world, n);
    let originators = originators(world, n, &links, &best);
    let neighbors: Vec<_> = links
        .iter()
        .map(|link| {
            json!({
                "hard_ifindex": link.ifindex,
                "hard_ifname": link.ifname,
                "last_seen_msecs": last_seen(world, link.to + 7),
                "neigh_address": link.neigh,
            })
        })
        .collect();
    let gateways = gateways(world, n, &links, &best);
    let (local, global) = translations(world, n);
    let mut hardifs = vec![hardif(MESH_IF, 12, &node.mesh_mac().to_string())];
    if node.wired {
        let port = if node.gateway {
            GATEWAY_BACKHAUL_PORT
        } else {
            node.model.wan_port
        };
        hardifs.push(hardif(port, 7, &node.wired_mac().to_string()));
    }
    json!({"bat0": {
        "mesh": {
            "version": node.release.kernel,
            "algo_name": "BATMAN_IV",
            "mesh_ifindex": 14,
            "mesh_ifname": "bat0",
            "mesh_address": node.mac_at(5).to_string(),
            "hard_ifname": MESH_IF,
            "hard_address": node.mesh_mac().to_string(),
            "gw_mode": if node.gateway { "server" } else { "client" },
            "gw_bandwidth_down": 1000,
            "gw_bandwidth_up": 200,
            "hop_penalty": HOP_PENALTY,
            "orig_interval": 1000,
            "bridge_loop_avoidance_enabled": true,
            "distributed_arp_table_enabled": true,
        },
        "hardifs": hardifs,
        "originators": originators,
        "neighbors": neighbors,
        "gateways": gateways,
        "global": global,
        "local": local,
    }})
}

fn hardif(name: &str, index: u32, address: &str) -> Value {
    json!({
        "mesh_ifindex": 14,
        "mesh_ifname": "bat0",
        "hard_ifindex": index,
        "hard_ifname": name,
        "hard_address": address,
        "active": true,
    })
}

fn translation(
    mac: &str,
    orig: Option<&str>,
    wifi: bool,
    nopurge: bool,
    last_seen: Option<u64>,
) -> Value {
    let mut entry = json!({
        "tt_address": mac,
        "tt_vid": -1,
        "tt_flags": {"del": false, "roam": false, "wifi": wifi, "isolated": false, "nopurge": nopurge, "new": false, "pending": false, "temp": false},
        "best": true,
    });
    if let Some(orig) = orig {
        entry["orig_address"] = json!(orig);
    }
    if let Some(last_seen) = last_seen {
        entry["last_seen_msecs"] = json!(last_seen);
    }
    entry
}
