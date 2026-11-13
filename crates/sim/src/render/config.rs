//! A node's `/etc/config` before uniwrt manages it: OpenWrt 25.12's stock packages, configured by
//! hand to run the simulated network (the gateway's LAN, WAN on VLAN 201 and DHCP; the access
//! points as bridges taking their address over DHCP, dnsmasq, odhcpd and the firewall off; the
//! sim's SSID on both bands and its batman-adv mesh on 5 GHz), so that the demo's review shows
//! what managing such a fleet changes. Section names and options follow OpenWrt's own, which
//! uniwrt's render names differently in places.

use std::{collections::BTreeMap, fmt::Write as _, net::Ipv4Addr};

use super::wifi::{MESH_ID, SSID, WIFI_PATH};
use crate::world::{
    BENCH_PORT, FACTORY_LAN, Node, SEGMENT_ADDRESS, World,
    catalog::{GATEWAY_BACKHAUL_PORT, WAN_VLAN},
};

/// The demo WLAN's passphrase: the demo's, so not secret.
pub const PASSPHRASE: &str = "uniwrt-demo-wifi";
/// The demo mesh's key.
pub const MESH_KEY: &str = "uniwrt-demo-mesh-key";

/// OpenWrt 25.12's stock firewall and DHCP (`crates/uci/fixtures/openwrt/25.12`).
const FIREWALL: &str = include_str!("../../data/config/firewall");
const DHCP: &str = include_str!("../../data/config/dhcp");

/// The init scripts a hand-made access point turns off.
pub(crate) const AP_OFF: [&str; 3] = ["dnsmasq", "firewall", "odhcpd"];

/// The node's packages, by name.
pub(crate) fn files(world: &World, n: usize) -> BTreeMap<String, String> {
    let node = &world.nodes[n];
    [
        ("system", system(node)),
        ("network", network(world, node)),
        ("wireless", wireless(node)),
        ("dhcp", dhcp(world, node)),
        ("firewall", firewall(world, node)),
    ]
    .into_iter()
    .map(|(name, text)| (name.to_owned(), text))
    .collect()
}

/// Whether `node` is the gateway of a world with a bench: it holds the adoption port's segment
/// as uniwrt's render writes it (`uniwrt_render::adoption`).
fn holds_segment(world: &World, node: &Node) -> bool {
    node.gateway && world.bench.is_some()
}

/// A factory-default gateway's packages (OpenWrt 25.12's first boot): the LAN at 192.168.1.1/24,
/// DHCP on the untagged WAN port, the radios off with the SSID `OpenWrt`.
pub(crate) fn factory_files(node: &Node) -> BTreeMap<String, String> {
    let o = node.mac.octets();
    let wan = node.model.wan_port;
    let mut network = format!(
        "\nconfig interface 'loopback'\n\toption device 'lo'\n\toption proto 'static'\n\
         \tlist ipaddr '127.0.0.1/8'\n\
         \nconfig globals 'globals'\n\
         \toption ula_prefix 'fd{:02x}:{:02x}{:02x}:{:02x}{:02x}::/48'\n\
         \nconfig device\n\toption name 'br-lan'\n\toption type 'bridge'\n",
        o[1], o[2], o[3], o[4], o[5],
    );
    for port in node.model.lan_ports {
        let _ = writeln!(network, "\tlist ports '{port}'");
    }
    let _ = write!(
        network,
        "\nconfig interface 'lan'\n\toption device 'br-lan'\n\toption proto 'static'\n\
         \tlist ipaddr '{FACTORY_LAN}/24'\n\toption ip6assign '60'\n\
         \nconfig interface 'wan'\n\toption device '{wan}'\n\toption proto 'dhcp'\n\
         \nconfig interface 'wan6'\n\toption device '{wan}'\n\toption proto 'dhcpv6'\n"
    );
    let mut wireless = String::new();
    for radio in &node.radios {
        let index = radio.band.index();
        let path = if index == 0 {
            WIFI_PATH.to_owned()
        } else {
            format!("{WIFI_PATH}+{index}")
        };
        let _ = write!(
            wireless,
            "\nconfig wifi-device 'radio{index}'\n\toption type 'mac80211'\n\toption path '{path}'\n\
             \toption band '{}'\n\toption channel '{}'\n\toption htmode 'HE{}'\n\
             \toption cell_density '0'\n\toption disabled '1'\n\
             \nconfig wifi-iface 'default_radio{index}'\n\toption device 'radio{index}'\n\
             \toption network 'lan'\n\toption mode 'ap'\n\toption ssid 'OpenWrt'\n\
             \toption encryption 'none'\n",
            radio.band.uci(),
            radio.channel,
            radio.band.width_mhz(),
        );
    }
    [
        ("system", system(node)),
        ("network", network),
        ("wireless", wireless),
        ("dhcp", DHCP.to_owned()),
        ("firewall", FIREWALL.to_owned()),
    ]
    .into_iter()
    .map(|(name, text)| (name.to_owned(), text))
    .collect()
}

/// The IPv4 addresses of `lan` in a node's `network`, in order: an applied renumbering holds the
/// new one first, beside the old (D-081).
pub(crate) fn lan_addresses(files: &BTreeMap<String, String>) -> Vec<Ipv4Addr> {
    static_addresses(files, "lan")
}

/// The IPv4 addresses of the static `interface` in a node's `network`, in order.
pub(crate) fn static_addresses(files: &BTreeMap<String, String>, interface: &str) -> Vec<Ipv4Addr> {
    let Some(network) = files
        .get("network")
        .and_then(|text| uniwrt_uci::parse("network", text).ok())
    else {
        return Vec::new();
    };
    network
        .section(interface)
        .filter(|lan| lan.kind == "interface" && lan.option("proto") == Some("static"))
        .and_then(|lan| lan.get("ipaddr"))
        .map(|value| {
            value
                .items()
                .iter()
                .filter_map(|item| item.split('/').next()?.parse().ok())
                .collect()
        })
        .unwrap_or_default()
}

/// The access points `wireless` sets up, as `(section, device, ssid)`, in its order.
pub(crate) fn aps(files: &BTreeMap<String, String>) -> Vec<(String, String, String)> {
    let Some(wireless) = files
        .get("wireless")
        .and_then(|text| uniwrt_uci::parse("wireless", text).ok())
    else {
        return Vec::new();
    };
    wireless
        .sections_of("wifi-iface")
        .filter(|iface| iface.option("mode") == Some("ap") && iface.option("disabled") != Some("1"))
        .filter_map(|iface| {
            Some((
                iface.name.clone()?,
                iface.option("device")?.to_owned(),
                iface.option("ssid")?.to_owned(),
            ))
        })
        .collect()
}

/// The `config` set: each file's SHA-256, as `node-agent/collect/config.uc` reports it.
pub(crate) fn hashes(files: &BTreeMap<String, String>) -> serde_json::Value {
    use sha2::{Digest as _, Sha256};
    let files: serde_json::Map<_, _> = files
        .iter()
        .map(|(name, text)| {
            let hash = data_encoding::HEXLOWER.encode(&Sha256::digest(text.as_bytes()));
            (name.clone(), serde_json::Value::String(hash))
        })
        .collect();
    serde_json::json!({ "files": files })
}

fn system(node: &Node) -> String {
    let mut out = String::new();
    let _ = write!(
        out,
        "\nconfig system\n\toption hostname '{}'\n\toption timezone 'GMT0'\n\toption zonename 'UTC'\n\
         \toption ttylogin '0'\n\toption log_size '128'\n\toption urandom_seed '0'\n",
        node.name
    );
    out.push_str("\nconfig timeserver 'ntp'\n\toption enabled '1'\n\toption enable_server '0'\n");
    for server in 0..4 {
        let _ = writeln!(out, "\tlist server '{server}.openwrt.pool.ntp.org'");
    }
    out
}

fn network(world: &World, node: &Node) -> String {
    let o = node.mac.octets();
    let mut out = format!(
        "\nconfig interface 'loopback'\n\toption device 'lo'\n\toption proto 'static'\n\
         \tlist ipaddr '127.0.0.1/8'\n\
         \nconfig globals 'globals'\n\
         \toption dhcp_default_duid '0004{}5e1d0c4a9b3f27a86d01'\n\
         \toption ula_prefix 'fd{:02x}:{:02x}{:02x}:{:02x}{:02x}::/48'\n",
        node.mac.to_string().replace(':', ""),
        o[1],
        o[2],
        o[3],
        o[4],
        o[5],
    );
    out.push_str("\nconfig device\n\toption name 'br-lan'\n\toption type 'bridge'\n");
    let segment = holds_segment(world, node);
    for port in node.model.lan_ports {
        if !(segment && *port == BENCH_PORT) {
            let _ = writeln!(out, "\tlist ports '{port}'");
        }
    }
    // An access point without its wired backhaul bridges its WAN port too.
    if !node.gateway && !node.wired {
        let _ = writeln!(out, "\tlist ports '{}'", node.model.wan_port);
    }
    out.push_str("\tlist ports 'bat0'\n");
    if node.gateway {
        let mask = super::net::lan_mask(world);
        let wan = node.model.wan_port;
        let _ = write!(
            out,
            "\nconfig interface 'lan'\n\toption device 'br-lan'\n\toption proto 'static'\n\
             \tlist ipaddr '{}/{mask}'\n\toption ip6assign '60'\n\
             \nconfig device\n\toption type '8021q'\n\toption ifname '{wan}'\n\
             \toption vid '{WAN_VLAN}'\n\toption name '{wan}.{WAN_VLAN}'\n\
             \nconfig interface 'wan'\n\toption device '{wan}.{WAN_VLAN}'\n\toption proto 'dhcp'\n\
             \nconfig interface 'wan6'\n\toption device '{wan}.{WAN_VLAN}'\n\
             \toption proto 'dhcpv6'\n",
            node.ip
        );
        if segment {
            let _ = write!(
                out,
                "\nconfig device\n\toption name 'br-adopt'\n\toption type 'bridge'\n\
                 \tlist ports '{BENCH_PORT}'\n\toption ipv6 '0'\n\
                 \nconfig interface 'uw_adopt'\n\toption device 'br-adopt'\n\
                 \toption proto 'static'\n\tlist ipaddr '{SEGMENT_ADDRESS}/24'\n"
            );
        }
    } else {
        out.push_str("\nconfig interface 'lan'\n\toption device 'br-lan'\n\toption proto 'dhcp'\n");
    }
    let _ = write!(
        out,
        "\nconfig interface 'bat0'\n\toption proto 'batadv'\n\toption routing_algo 'BATMAN_IV'\n\
         \toption gw_mode '{}'\n\toption bridge_loop_avoidance '1'\n\
         \nconfig interface 'batmesh'\n\toption proto 'batadv_hardif'\n\toption master 'bat0'\n",
        if node.gateway { "server" } else { "client" }
    );
    if node.wired {
        let port = if node.gateway {
            GATEWAY_BACKHAUL_PORT
        } else {
            node.model.wan_port
        };
        let _ = write!(
            out,
            "\nconfig interface 'batwired'\n\toption proto 'batadv_hardif'\n\
             \toption master 'bat0'\n\toption device '{port}'\n"
        );
    }
    out
}

fn wireless(node: &Node) -> String {
    let mut out = String::new();
    for radio in &node.radios {
        let index = radio.band.index();
        let path = if index == 0 {
            WIFI_PATH.to_owned()
        } else {
            format!("{WIFI_PATH}+{index}")
        };
        let _ = write!(
            out,
            "\nconfig wifi-device 'radio{index}'\n\toption type 'mac80211'\n\toption path '{path}'\n\
             \toption band '{}'\n\toption channel '{}'\n\toption htmode 'HE{}'\n\
             \toption country 'US'\n\toption cell_density '0'\n\
             \nconfig wifi-iface 'default_radio{index}'\n\toption device 'radio{index}'\n\
             \toption network 'lan'\n\toption mode 'ap'\n\toption ssid '{SSID}'\n\
             \toption encryption 'sae-mixed'\n\toption key '{PASSPHRASE}'\n\
             \toption ieee80211r '1'\n\toption ft_over_ds '0'\n\toption ft_psk_generate_local '1'\n",
            radio.band.uci(),
            radio.channel,
            radio.band.width_mhz(),
        );
    }
    let _ = write!(
        out,
        "\nconfig wifi-iface 'mesh_radio1'\n\toption device 'radio1'\n\toption network 'batmesh'\n\
         \toption mode 'mesh'\n\toption mesh_id '{MESH_ID}'\n\toption encryption 'sae'\n\
         \toption key '{MESH_KEY}'\n\toption mesh_fwding '0'\n"
    );
    out
}

/// The gateway's is stock (and serves nothing on the adoption port); an access point's serves
/// no DHCP on its LAN.
fn dhcp(world: &World, node: &Node) -> String {
    if holds_segment(world, node) {
        return format!(
            "{DHCP}\nconfig dhcp 'uw_adopt'\n\toption interface 'uw_adopt'\n\toption ignore '1'\n"
        );
    }
    if node.gateway {
        return DHCP.to_owned();
    }
    let dnsmasq = DHCP.split("\nconfig dhcp 'lan'").next().unwrap_or(DHCP);
    format!(
        "{dnsmasq}\nconfig dhcp 'lan'\n\toption interface 'lan'\n\toption ignore '1'\n\
         \nconfig odhcpd 'odhcpd'\n\toption maindhcp '0'\n\toption leasefile '/tmp/odhcpd.leases'\n\
         \toption leasetrigger '/usr/sbin/odhcpd-update'\n\toption loglevel '4'\n"
    )
}

/// Stock, with the adoption port's zone on a gateway that holds it.
fn firewall(world: &World, node: &Node) -> String {
    if !holds_segment(world, node) {
        return FIREWALL.to_owned();
    }
    format!(
        "{FIREWALL}\nconfig zone\n\toption name 'uw_adopt'\n\tlist network 'uw_adopt'\n\
         \toption input 'REJECT'\n\toption output 'ACCEPT'\n\toption forward 'REJECT'\n"
    )
}

#[cfg(test)]
mod tests {
    use uniwrt_core::Timestamp;

    use super::*;
    use crate::SimConfig;

    #[test]
    fn every_package_parses_and_describes_the_simulated_network() {
        let world = World::new(
            &SimConfig::default(),
            Timestamp::from_unix_ms(1_793_245_539_000),
        );
        for n in 0..world.nodes.len() {
            let node = &world.nodes[n];
            let files = files(&world, n);
            for (name, text) in &files {
                uniwrt_uci::parse(name, text)
                    .unwrap_or_else(|err| panic!("{} {name}: {err}\n{text}", node.name));
            }
            assert!(files["system"].contains(&format!("hostname '{}'", node.name)));
            assert!(files["wireless"].contains(&format!("ssid '{SSID}'")));
            assert!(files["wireless"].contains(&format!("mesh_id '{MESH_ID}'")));
            let network = &files["network"];
            assert_eq!(
                network.contains("proto 'static'\n\tlist ipaddr '10.0.0.1/"),
                node.gateway
            );
            assert_eq!(network.contains(&format!(".{WAN_VLAN}'")), node.gateway);
            assert_eq!(network.contains("'batwired'"), node.wired);
            let lan_off = "config dhcp 'lan'\n\toption interface 'lan'\n\toption ignore '1'";
            assert_eq!(files["dhcp"].contains(lan_off), !node.gateway);
        }
    }
}
