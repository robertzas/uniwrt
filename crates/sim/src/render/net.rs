//! The `interfaces` set: netifd's `network.interface dump`.

use serde_json::{Value, json};

use crate::world::{
    World,
    catalog::{GATEWAY_BACKHAUL_PORT, WAN_VLAN},
};

/// The gateway's public addresses (documentation ranges).
const WAN_IP: &str = "203.0.113.57";
const WAN_ROUTER: &str = "203.0.113.1";
const WAN_DNS: [&str; 2] = ["203.0.113.53", "203.0.113.54"];
pub(super) const LAN_PREFIX6: &str = "2001:db8:4a:100::";

pub(super) fn interfaces(world: &World, n: usize) -> Value {
    let node = &world.nodes[n];
    let uptime = ((world.now_ms() - node.booted_ms) / 1000 - 20).max(1);
    let mask = if world.nodes[0].ip.octets()[2] == 1 {
        24
    } else {
        16
    };
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
        let vlan = format!("{}.{WAN_VLAN}", node.model.wan_port);
        list.push(iface("wan", "dhcp", &vlan, uptime, json!({
            "ipv4-address": [{"address": WAN_IP, "mask": 24}],
            "route": [{"target": "0.0.0.0", "mask": 0, "nexthop": WAN_ROUTER, "source": format!("{WAN_IP}/32")}],
            "dns-server": WAN_DNS,
            "data": {"leasetime": 86_400},
        })));
        list.push(iface("wan6", "dhcpv6", &vlan, uptime, json!({
            "ipv6-address": [{"address": "2001:db8:4a:ff::57", "mask": 128}],
            "ipv6-prefix": [{"address": "2001:db8:4a:100::", "mask": 56}],
            "route": [{"target": "::", "mask": 0, "nexthop": "fe80::1", "source": "2001:db8:4a:100::/56"}],
            "dns-server": ["2001:db8:4a::53"],
        })));
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
    json!({"dump": {"interface": list}})
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
