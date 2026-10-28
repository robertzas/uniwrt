//! The `batman` set: batctl's JSON queries per mesh device. Throughputs and gateway bandwidths
//! are in units of 100 kbit/s; a translation-table VLAN of -1 means untagged. Of the global
//! table, only the routes the mesh uses (`best`) are kept; of the local one, the node's own
//! interfaces (`nopurge`) are left out.

use std::collections::BTreeMap;

use serde::Deserialize;
use uniwrt_core::MacAddr;
use uniwrt_model::facts::{
    BatmanAlgorithm, BatmanFacts, BatmanGateway, BatmanNeighbor, GatewayMode, HardIf, LocalClient,
    Originator, RemoteClient,
};

pub(super) type Raw = BTreeMap<String, Mesh>;

#[derive(Deserialize)]
pub(super) struct Mesh {
    #[serde(rename = "mesh")]
    info: MeshInfo,
    #[serde(default)]
    hardifs: Option<Vec<HardIfEntry>>,
    #[serde(default)]
    originators: Option<Vec<Route>>,
    #[serde(default)]
    neighbors: Option<Vec<Route>>,
    #[serde(default)]
    gateways: Option<Vec<Gateway>>,
    #[serde(default)]
    global: Option<Vec<Translation>>,
    #[serde(default)]
    local: Option<Vec<Translation>>,
}

#[derive(Deserialize)]
struct MeshInfo {
    mesh_address: Option<String>,
    algo_name: Option<String>,
    gw_mode: Option<String>,
}

#[derive(Deserialize)]
struct HardIfEntry {
    hard_ifname: String,
    hard_address: Option<String>,
    #[serde(default)]
    active: bool,
}

/// An originator (`oj`) or a neighbor (`nj`).
#[derive(Deserialize)]
struct Route {
    hard_ifname: String,
    orig_address: Option<String>,
    neigh_address: String,
    #[serde(default)]
    last_seen_msecs: u64,
    tq: Option<u8>,
    throughput: Option<u32>,
    #[serde(default)]
    best: bool,
}

#[derive(Deserialize)]
struct Gateway {
    orig_address: String,
    router: String,
    hard_ifname: String,
    tq: Option<u8>,
    throughput: Option<u32>,
    bandwidth_down: Option<u32>,
    bandwidth_up: Option<u32>,
    #[serde(default)]
    best: bool,
}

#[derive(Deserialize)]
struct Translation {
    tt_address: String,
    tt_vid: Option<i32>,
    orig_address: Option<String>,
    #[serde(default)]
    tt_flags: TtFlags,
    last_seen_msecs: Option<u64>,
    #[serde(default)]
    best: bool,
}

#[derive(Default, Deserialize)]
struct TtFlags {
    #[serde(default)]
    wifi: bool,
    #[serde(default)]
    roam: bool,
    #[serde(default)]
    nopurge: bool,
}

pub(super) fn parse(raw: Raw) -> Vec<BatmanFacts> {
    raw.into_iter()
        .map(|(mesh_if, mesh)| BatmanFacts {
            mesh_if,
            mac: mac(mesh.info.mesh_address.as_deref()),
            algorithm: match mesh.info.algo_name.as_deref() {
                Some("BATMAN_IV") => Some(BatmanAlgorithm::BatmanIv),
                Some("BATMAN_V") => Some(BatmanAlgorithm::BatmanV),
                _ => None,
            },
            gw_mode: match mesh.info.gw_mode.as_deref() {
                Some("off") => Some(GatewayMode::Off),
                Some("client") => Some(GatewayMode::Client),
                Some("server") => Some(GatewayMode::Server),
                _ => None,
            },
            hard_ifs: list(mesh.hardifs, |entry| {
                Some(HardIf {
                    name: entry.hard_ifname,
                    mac: mac(entry.hard_address.as_deref()),
                    active: entry.active,
                })
            }),
            originators: list(mesh.originators, |route| {
                Some(Originator {
                    mac: mac(route.orig_address.as_deref())?,
                    next_hop: route.neigh_address.parse().ok()?,
                    hard_if: route.hard_ifname,
                    last_seen_ms: route.last_seen_msecs,
                    tq: route.tq,
                    throughput_kbps: kbps(route.throughput),
                    best: route.best,
                })
            }),
            neighbors: list(mesh.neighbors, |route| {
                Some(BatmanNeighbor {
                    mac: route.neigh_address.parse().ok()?,
                    hard_if: route.hard_ifname,
                    last_seen_ms: route.last_seen_msecs,
                    throughput_kbps: kbps(route.throughput),
                })
            }),
            gateways: list(mesh.gateways, |gateway| {
                Some(BatmanGateway {
                    originator: gateway.orig_address.parse().ok()?,
                    next_hop: gateway.router.parse().ok()?,
                    hard_if: gateway.hard_ifname,
                    tq: gateway.tq,
                    throughput_kbps: kbps(gateway.throughput),
                    down_kbps: kbps(gateway.bandwidth_down),
                    up_kbps: kbps(gateway.bandwidth_up),
                    best: gateway.best,
                })
            }),
            remote_clients: list(mesh.global, |entry| {
                entry.best.then_some(())?;
                Some(RemoteClient {
                    mac: entry.tt_address.parse().ok()?,
                    vid: vid(entry.tt_vid),
                    originator: mac(entry.orig_address.as_deref())?,
                    wifi: entry.tt_flags.wifi,
                    roaming: entry.tt_flags.roam,
                })
            }),
            local_clients: list(mesh.local, |entry| {
                (!entry.tt_flags.nopurge).then_some(())?;
                Some(LocalClient {
                    mac: entry.tt_address.parse().ok()?,
                    vid: vid(entry.tt_vid),
                    wifi: entry.tt_flags.wifi,
                    last_seen_ms: entry.last_seen_msecs,
                })
            }),
        })
        .collect()
}

/// The entries of a query that parse; none when the query failed.
fn list<T, U>(entries: Option<Vec<T>>, f: impl FnMut(T) -> Option<U>) -> Vec<U> {
    entries
        .unwrap_or_default()
        .into_iter()
        .filter_map(f)
        .collect()
}

fn mac(text: Option<&str>) -> Option<MacAddr> {
    text?.parse().ok()
}

fn kbps(hundreds: Option<u32>) -> Option<u32> {
    hundreds?.checked_mul(100)
}

fn vid(vid: Option<i32>) -> Option<u16> {
    u16::try_from(vid?).ok()
}
