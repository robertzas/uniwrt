//! The `devices` set: each kernel device's sysfs attributes, and its counters in
//! /proc/net/dev. A gateway is a DSA switch (`lan1`–`lan5` on the conduit `eth0`) with its
//! uplink on `eth1` and the ISP's VLAN on top; an access point has a WAN and a LAN port.

use std::fmt::Write as _;

use serde_json::{Map, Value, json};
use uniwrt_core::MacAddr;

use crate::world::{
    Counters, Node, World,
    catalog::{GATEWAY_BACKHAUL_PORT, WAN_VLAN},
};

/// One kernel device.
struct Dev {
    name: String,
    /// `ARPHRD_*`: 1 for Ethernet, 772 for loopback.
    arphrd: u32,
    mac: MacAddr,
    mtu: u32,
    carrier: bool,
    /// The negotiated Mbit/s of a port with a link.
    speed: Option<u32>,
    /// The uevent's `DEVTYPE`, if any.
    devtype: &'static str,
    master: Option<&'static str>,
    physical: bool,
    wireless: bool,
    counters: Counters,
}

impl Dev {
    fn new(
        name: impl Into<String>,
        mac: MacAddr,
        devtype: &'static str,
        counters: Counters,
    ) -> Self {
        Self {
            name: name.into(),
            arphrd: 1,
            mac,
            mtu: 1500,
            carrier: true,
            speed: None,
            devtype,
            master: None,
            physical: false,
            wireless: false,
            counters,
        }
    }

    /// A physical port, with a link at `speed` while `carrier`.
    fn port(self, carrier: bool, speed: u32) -> Self {
        Self {
            carrier,
            speed: carrier.then_some(speed),
            physical: true,
            ..self
        }
    }

    fn wireless(self) -> Self {
        Self {
            physical: true,
            wireless: true,
            ..self
        }
    }

    fn master(self, master: &'static str) -> Self {
        Self {
            master: Some(master),
            ..self
        }
    }

    fn sys(&self) -> Value {
        let devtype = if self.devtype.is_empty() {
            String::new()
        } else {
            format!("DEVTYPE={}\n", self.devtype)
        };
        let (speed, duplex) = match (self.wireless, self.speed) {
            (true, _) => (Value::Null, Value::Null),
            (false, Some(speed)) => (json!(speed.to_string()), json!("full")),
            (false, None) => (json!("-1"), json!("unknown")),
        };
        json!({
            "type": self.arphrd.to_string(),
            "address": self.mac.to_string(),
            "mtu": self.mtu.to_string(),
            "flags": if self.arphrd == 772 { "0x9" } else { "0x1003" },
            "carrier": if self.carrier { "1" } else { "0" },
            "speed": speed,
            "duplex": duplex,
            "operstate": if self.carrier { "up" } else { "down" },
            "uevent": format!("{devtype}INTERFACE={}", self.name),
            "master": self.master,
            "wireless": self.wireless,
            "physical": self.physical,
        })
    }
}

pub(super) fn devices(world: &World, n: usize) -> Value {
    let node = &world.nodes[n];
    let port = |name: &str| node.ports.get(name).copied().unwrap_or_default();
    let wired: Vec<&str> = world
        .clients
        .iter()
        .filter_map(|client| client.port.filter(|(at, _)| *at == n).map(|(_, port)| port))
        .collect();
    let mut devs = vec![Dev {
        arphrd: 772,
        mtu: 65_536,
        ..Dev::new("lo", MacAddr::default(), "", loopback(world, node))
    }];
    let lan_devtype = if node.gateway { "dsa" } else { "" };
    for &name in node.model.lan_ports {
        let dev = Dev::new(name, node.mac, lan_devtype, port(name));
        devs.push(dev.port(wired.contains(&name), 1000).master("br-lan"));
    }
    let lan = sum(node.model.lan_ports.iter().map(|name| port(name)));
    let wan = node.model.wan_port;
    let uplink =
        Dev::new(wan, node.mac_at(1), "", port(wan)).port(node.gateway || node.wired, 2500);
    devs.push(if node.gateway || !node.wired {
        uplink
    } else {
        uplink.master("bat0")
    });
    if node.gateway {
        devs.push(Dev::new(
            format!("{wan}.{WAN_VLAN}"),
            node.mac_at(1),
            "vlan",
            port(wan),
        ));
        devs.push(Dev {
            mtu: 1504,
            ..Dev::new("eth0", node.mac, "", lan).port(true, 10_000)
        });
        let backhaul = Dev::new(
            GATEWAY_BACKHAUL_PORT,
            node.mac,
            "dsa",
            port(GATEWAY_BACKHAUL_PORT),
        );
        devs.push(backhaul.port(node.wired, 1000).master("bat0"));
    }
    let mut bridged = lan;
    for radio in &node.radios {
        let iface = radio.band.ap_iface();
        devs.push(
            Dev::new(iface, node.bssid(radio.band), "wlan", radio.ap)
                .wireless()
                .master("br-lan"),
        );
        bridged = sum([bridged, radio.ap]);
    }
    let mesh = node.radios[1].mesh;
    devs.push(
        Dev::new("phy1-mesh0", node.mesh_mac(), "wlan", mesh)
            .wireless()
            .master("bat0"),
    );
    let backhaul = match (node.wired, node.gateway) {
        (false, _) => Counters::default(),
        (true, true) => port(GATEWAY_BACKHAUL_PORT),
        (true, false) => port(wan),
    };
    let bat = sum([mesh, backhaul]);
    devs.push(Dev::new("bat0", node.mac_at(5), "", bat).master("br-lan"));
    devs.push(Dev::new("br-lan", node.mac, "bridge", sum([bridged, bat])));
    render(&devs)
}

fn render(devs: &[Dev]) -> Value {
    let mut net_dev = String::from(concat!(
        "Inter-|   Receive                                                |  Transmit\n",
        " face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed\n",
    ));
    let mut sys = Map::new();
    for dev in devs {
        let c = dev.counters;
        let _ = writeln!(
            net_dev,
            "{:>6}: {} {} 0 0 0 0 0 {} {} {} 0 0 0 0 0 0",
            dev.name,
            c.rx_bytes,
            c.rx_packets,
            c.rx_packets / 50,
            c.tx_bytes,
            c.tx_packets
        );
        sys.insert(dev.name.clone(), dev.sys());
    }
    json!({"sys": sys, "net_dev": net_dev})
}

fn loopback(world: &World, node: &Node) -> Counters {
    let mut counters = Counters::default();
    let uptime = ((world.now_ms() - node.booted_ms) / 1000).max(0) as u64;
    counters.add(uptime * 40, uptime * 40);
    counters
}

fn sum(parts: impl IntoIterator<Item = Counters>) -> Counters {
    parts
        .into_iter()
        .fold(Counters::default(), |total, part| Counters {
            rx_bytes: total.rx_bytes + part.rx_bytes,
            tx_bytes: total.tx_bytes + part.tx_bytes,
            rx_packets: total.rx_packets + part.rx_packets,
            tx_packets: total.tx_packets + part.tx_packets,
        })
}
