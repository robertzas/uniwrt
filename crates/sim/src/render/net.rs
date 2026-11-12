//! The `interfaces` set: netifd's `network.interface dump`.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::{
    isp,
    world::{World, catalog::GATEWAY_BACKHAUL_PORT},
};

/// The ISP's resolvers (documentation ranges).
const WAN_DNS: [&str; 2] = ["203.0.113.53", "203.0.113.54"];
pub(super) const LAN_PREFIX6: &str = "2001:db8:4a:100::";

/// The LAN's prefix length: a /24 while the fleet fits, else a /16.
pub(super) fn lan_mask(world: &World) -> u8 {
    if world.wide { 16 } else { 24 }
}

/// The interfaces; the gateway's WAN as the ISP answers its `/etc/config/network` (`files`).
pub(super) fn interfaces(world: &World, n: usize, files: &BTreeMap<String, String>) -> Value {
    let node = &world.nodes[n];
    let uptime = ((world.now_ms() - node.booted_ms) / 1000 - 20).max(1);
    let mask = lan_mask(world);
    let gateway_ip = world.nodes[0].ip.to_string();
    let mut list = vec![iface(
        "loopback",
        "static",
        "lo",
        uptime,
        json!({"ipv4-address": [{"address": "127.0.0.1", "mask": 8}]}),
    )];
    if node.gateway {
        list.push(iface("lan", "static", "br-lan", uptime, json!({
            "ipv4-address": [{"address": node.ip.to_string(), "mask": mask}],
            "ipv6-prefix-assignment": [{"address": LAN_PREFIX6, "mask": 64, "local-address": {"address": format!("{LAN_PREFIX6}1"), "mask": 64}}],
        })));
        let uplink = isp::uplink(
            files.get("network").map(String::as_str),
            node.model.wan_port,
        );
        list.extend(wan(&uplink, uptime));
    } else {
        list.push(iface("lan", "dhcp", "br-lan", uptime, json!({
            "ipv4-address": [{"address": node.ip.to_string(), "mask": mask}],
            "route": [{"target": "0.0.0.0", "mask": 0, "nexthop": gateway_ip, "source": format!("{}/32", node.ip)}],
            "dns-server": [gateway_ip],
        })));
    }
    list.push(iface("bat0", "batadv", "bat0", uptime, json!({})));
    list.push(iface(
        "batmesh",
        "batadv_hardif",
        "phy1-mesh0",
        uptime,
        json!({}),
    ));
    if node.wired {
        let port = if node.gateway {
            GATEWAY_BACKHAUL_PORT
        } else {
            node.model.wan_port
        };
        list.push(iface("batwired", "batadv_hardif", port, uptime, json!({})));
    }
    for vlan in &node.wiring.vlans {
        list.push(iface(
            &vlan.name,
            "batadv_hardif",
            &vlan.dev(),
            uptime,
            json!({}),
        ));
    }
    json!({"dump": {"interface": list}})
}

/// The gateway's `wan` and `wan6`, up or waiting as the ISP answered.
fn wan(uplink: &isp::Uplink, uptime: i64) -> Vec<Value> {
    let mut list = Vec::new();
    let mut device = String::new();
    if let Some((proto, dev, up)) = &uplink.wan {
        device.clone_from(dev);
        let mut wan = iface(
            "wan",
            proto,
            dev,
            uptime,
            if *up {
                json!({
                    "ipv4-address": [{"address": isp::LEASE, "mask": 24}],
                    "route": [{"target": "0.0.0.0", "mask": 0, "nexthop": isp::ROUTER, "source": format!("{}/32", isp::LEASE)}],
                    "dns-server": WAN_DNS,
                    "data": {"leasetime": 86_400},
                })
            } else {
                json!({"up": false, "uptime": 0, "pending": true})
            },
        );
        if !*up && proto == "pppoe" {
            wan["errors"] = json!([{"subsystem": "pppoe", "code": "NO_PADO"}]);
        }
        list.push(wan);
    }
    if let Some((proto, up)) = &uplink.wan6 {
        let extra = match (proto.as_str(), *up) {
            ("dhcpv6", true) => json!({
                "ipv6-address": [{"address": "2001:db8:4a:ff::57", "mask": 128}],
                "ipv6-prefix": [{"address": LAN_PREFIX6, "mask": 56}],
                "route": [{"target": "::", "mask": 0, "nexthop": "fe80::1", "source": "2001:db8:4a:100::/56"}],
                "dns-server": ["2001:db8:4a::53"],
            }),
            ("6rd", true) => json!({
                "l3_device": "6rd-wan6",
                "ipv6-address": [{"address": "2602:cb00:7139::1", "mask": 64}],
                "ipv6-prefix": [{"address": "2602:cb00:7139::", "mask": 56}],
                "route": [{"target": "::", "mask": 0, "nexthop": "::", "source": "2602:cb00:7139::/56"}],
            }),
            _ => json!({"up": false, "uptime": 0, "pending": true}),
        };
        list.push(iface("wan6", proto, &device, uptime, extra));
    }
    list
}

fn iface(name: &str, proto: &str, device: &str, uptime: i64, extra: Value) -> Value {
    let mut iface = json!({
        "interface": name,
        "up": true,
        "pending": false,
        "available": true,
        "autostart": true,
        "dynamic": false,
        "uptime": uptime,
        "l3_device": device,
        "proto": proto,
        "device": device,
        "metric": 0,
        "dns_metric": 0,
        "delegation": true,
        "ipv4-address": [],
        "ipv6-address": [],
        "ipv6-prefix": [],
        "ipv6-prefix-assignment": [],
        "route": [],
        "dns-server": [],
        "dns-search": [],
        "neighbors": [],
        "data": {},
    });
    if let (Some(iface), Value::Object(extra)) = (iface.as_object_mut(), extra) {
        iface.extend(extra);
    }
    iface
}
