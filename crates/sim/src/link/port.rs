//! The adoption port's commands on the gateway (features/adoption.md, Path C; `world/bench.rs`):
//! netifd's status of the segment's interface, the lend script (`node-agent/adoption/lend.sh`)
//! and the ARP table, answered as `uniwrt_node::adoption_port` reads them; and where the bench
//! device answers on the segment.

use std::{collections::BTreeMap, fmt::Write as _, net::Ipv4Addr};

use serde_json::json;
use uniwrt_node::{ExecOutput, ExecRequest};

use crate::{
    render::{
        commands::{failed, ok},
        config,
    },
    world::{LendReply, World},
};

/// The segment's interface and bridge, as render names them.
const INTERFACE: &str = "uw_adopt";
const BRIDGE: &str = "br-adopt";

/// The gateway's address on the segment, if its `network` holds one.
pub(crate) fn segment_address(files: &BTreeMap<String, String>) -> Option<Ipv4Addr> {
    config::static_addresses(files, INTERFACE).first().copied()
}

/// Where the bench device answers on the segment now: its static LAN addresses, while it is up,
/// the gateway (whose files are `gateway`) holds the segment and the port isn't lent.
pub(crate) fn bench_hosts(
    world: &World,
    gateway: &BTreeMap<String, String>,
    device: &BTreeMap<String, String>,
) -> Vec<Ipv4Addr> {
    let Some(bench) = &world.bench else {
        return Vec::new();
    };
    if !world.nodes[bench.node].up() || world.port_lent() || segment_address(gateway).is_none() {
        return Vec::new();
    }
    config::lan_addresses(device)
}

/// The gateway's answer to `request`, if it is one of the port's commands. `device` is the
/// bench device's files.
pub(crate) fn exec(
    world: &mut World,
    gateway: &BTreeMap<String, String>,
    device: Option<&BTreeMap<String, String>>,
    request: &ExecRequest,
) -> Option<ExecOutput> {
    let argv: Vec<&str> = request.argv.iter().map(String::as_str).collect();
    Some(match argv.as_slice() {
        ["ubus", "call", "network.interface.uw_adopt", "status"] => {
            let Some(address) = segment_address(gateway) else {
                return Some(failed(4, "Command failed: Not found\n".to_owned()));
            };
            // The lend takes the interface down (`ifdown`) to free the port.
            let up = !world.port_lent();
            let addresses = if up {
                json!([{"address": address.to_string(), "mask": 24}])
            } else {
                json!([])
            };
            let status = json!({"up": up, "device": BRIDGE, "ipv4-address": addresses});
            ok(status.to_string().into_bytes())
        }
        ["cat", "/proc/net/arp"] => {
            let mut table =
                "IP address       HW type     Flags       HW address            Mask     Device\n"
                    .to_owned();
            if let (Some(bench), Some(device)) = (&world.bench, device) {
                let mac = world.nodes[bench.node].mac;
                for ip in bench_hosts(world, gateway, device) {
                    let _ = writeln!(
                        table,
                        "{ip:<16} 0x1         0x2         {mac}     *        {BRIDGE}"
                    );
                }
            }
            ok(table.into_bytes())
        }
        ["sh", "-c", script, "sh", verb, port, rest @ ..] if script.contains("uniwrt-lend") => {
            let seconds = match (*verb, rest) {
                ("lend", [_interface, seconds]) => seconds.parse().unwrap_or(0),
                _ => 0,
            };
            match world.lend(verb, port, seconds) {
                Some(LendReply::Lent) => ok(b"lent\n".to_vec()),
                Some(LendReply::Busy) => ok(b"busy\n".to_vec()),
                Some(LendReply::Kept) => ok(b"kept\n".to_vec()),
                Some(LendReply::Ended) => ok(b"ended\n".to_vec()),
                Some(LendReply::NotLent) => failed(1, format!("{port} is not lent\n")),
                None => failed(2, "bad port\n".to_owned()),
            }
        }
        _ => return None,
    })
}
