//! The `services` set as the sim's gateway reports it (D-132): each enabled DDNS section of its
//! `ddns` file registered with the ISP's lease (IPv4 only; the sim's WAN has no IPv6 address),
//! updated a minute after boot, its updater running while the `ddns` service is on; and one
//! https-dns-proxy instance per section of its package while that service is on; and the DNS
//! blocklist's files the controller uploaded to its `/tmp` since it booted (D-133); and the
//! WireGuard tunnel its `network` file sets up ([`super::wireguard`], D-140), and Tailscale
//! where its `firewall` file has the zone ([`super::tailscale`]).

use std::collections::BTreeMap;

use serde_json::{Value, json};

use uniwrt_model::site::blocking::{BLOCKLIST_CONF, BLOCKLIST_FILE};

use super::tmp::Tmp;
use crate::{isp, world::World};

pub(super) fn services(
    world: &World,
    n: usize,
    files: &BTreeMap<String, String>,
    running: &BTreeMap<String, bool>,
    tmp: &Tmp,
) -> Value {
    let node = &world.nodes[n];
    let now = world.now_ms() / 1000;
    let uptime = (world.now_ms() - node.booted_ms) / 1000;
    let on = |service: &str| running.get(service) == Some(&true);
    let parse = |name: &str| {
        files
            .get(name)
            .and_then(|text| uniwrt_uci::parse(name, text).ok())
    };
    let ddns: Vec<Value> = parse("ddns").map_or_default(|package| {
        package
            .sections_of("service")
            .filter_map(|s| {
                let id = s.name.clone()?;
                let live = on("ddns") && s.option("enabled") == Some("1");
                let v4 = s.option("use_ipv6") != Some("1");
                let registered = live && v4 && uptime > 60;
                Some(json!({
                    "id": id,
                    "ip": registered.then_some(isp::LEASE),
                    "update": registered.then(|| "60".to_owned()),
                    "running": live,
                }))
            })
            .collect()
    });
    let doh = parse("https-dns-proxy").map(|package| {
        if on("https-dns-proxy") {
            package.sections_of("https-dns-proxy").count()
        } else {
            0
        }
    });
    let head = tmp.file(node.booted_ms, BLOCKLIST_FILE).map(|text| {
        let line = text.split(|&b| b == b'\n').next().unwrap_or_default();
        String::from_utf8_lossy(line).into_owned()
    });
    let conf = tmp.file(node.booted_ms, BLOCKLIST_CONF).is_some();
    let network = parse("network");
    let wireguard = super::wireguard::wireguard(network.as_ref(), now, uptime);
    let tailscale = super::tailscale::tailscale(parse("firewall").as_ref());
    json!({
        "now": now, "uptime": uptime, "ddns": ddns, "doh": doh,
        "blocklist": head, "blocklist_conf": conf, "wireguard": wireguard,
        "tailscale": tailscale,
    })
}
