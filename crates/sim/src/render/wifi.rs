//! The sets `radios` (netifd's wireless status, nl80211 interfaces and surveys), `stations`
//! (nl80211 station dumps and hostapd's clients) and `phys` (a two-band MT7986, from the hwsim
//! capture's wiphys).

use std::sync::LazyLock;

use serde_json::{Map, Value, json};

use crate::world::{
    Assoc, Client, World,
    catalog::Standard,
    radio::{Band, Rate},
};

pub(crate) const SSID: &str = "uniwrt-demo";
const MESH_ID: &str = "uniwrt-demo-mesh";
/// The SoC's Wi-Fi device; its second phy is `+1`.
const WIFI_PATH: &str = "platform/soc/18000000.wifi";

static PHYS: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../data/phys.json")).unwrap_or(Value::Null)
});

pub(super) fn phys() -> Value {
    PHYS.clone()
}

pub(super) fn radios(world: &World, n: usize) -> Value {
    let node = &world.nodes[n];
    let mut wireless = Map::new();
    let mut nl = Vec::new();
    let mut surveys = Map::new();
    for radio in &node.radios {
        let band = radio.band;
        let index = band.index();
        let freq = band.freq(radio.channel);
        let htmode = format!("HE{}", band.width_mhz());
        let path = if index == 0 {
            WIFI_PATH.to_owned()
        } else {
            format!("{WIFI_PATH}+{index}")
        };
        let mut ifaces = vec![json!({
            "section": format!("default_radio{index}"),
            "ifname": band.ap_iface(),
            "config": {"mode": "ap", "ssid": SSID, "encryption": "sae-mixed", "network": ["lan"], "ieee80211r": true},
        })];
        let width = if band.width_mhz() == 80 { 3 } else { 1 };
        let nl_iface = |ifname: &str, iftype: u32, mac: String, ssid: Option<&str>| {
            let mut iface = json!({
                "ifname": ifname,
                "wiphy": index,
                "iftype": iftype,
                "mac": mac,
                "wiphy_freq": freq,
                "channel_width": width,
                "center_freq1": band.center_freq(radio.channel),
                "wiphy_tx_power_level": u32::from(radio.txpower) * 100,
            });
            if let Some(ssid) = ssid {
                iface["ssid"] = json!(ssid);
            }
            iface
        };
        nl.push(nl_iface(
            band.ap_iface(),
            3,
            node.bssid(band).to_string(),
            Some(SSID),
        ));
        if band == Band::G5 {
            ifaces.push(json!({
                "section": "mesh_radio1",
                "ifname": "phy1-mesh0",
                "config": {"mode": "mesh", "mesh_id": MESH_ID, "encryption": "sae", "network": ["batmesh"]},
            }));
            nl.push(nl_iface("phy1-mesh0", 7, node.mesh_mac().to_string(), None));
        }
        wireless.insert(
            format!("radio{index}"),
            json!({
                "up": true,
                "pending": false,
                "disabled": false,
                "config": {"band": band.uci(), "channel": radio.channel.to_string(), "htmode": htmode, "country": "US", "path": path},
                "interfaces": ifaces,
            }),
        );
        // The survey's counters run since the radio came up; only the ratio matters.
        let time = ((world.now_ms() - node.booted_ms) / 1000).max(1) as u64 * 1000;
        let busy = (time as f64 * radio.busy / 100.0) as u64;
        surveys.insert(
            format!("phy{index}"),
            json!({
                "frequency": freq,
                "noise": radio.noise.round() as i64,
                "time": time,
                "busy": busy,
                "busy_percent": (radio.busy * 10.0).round() / 10.0,
            }),
        );
    }
    json!({
        "wireless": wireless,
        "phys": [
            {"name": "phy0", "index": 0, "device": format!("/sys/devices/{WIFI_PATH}")},
            {"name": "phy1", "index": 1, "device": format!("/sys/devices/{WIFI_PATH}")},
        ],
        "interfaces": nl,
        "surveys": surveys,
    })
}

pub(super) fn stations(world: &World, n: usize) -> Value {
    let now_ms = world.now_ms();
    let mut stations: Map<String, Value> = ["phy0-ap0", "phy1-ap0", "phy1-mesh0"]
        .into_iter()
        .map(|iface| (iface.to_owned(), json!([])))
        .collect();
    let mut clients: Map<String, Value> = ["phy0-ap0", "phy1-ap0"]
        .into_iter()
        .map(|iface| (iface.to_owned(), json!({})))
        .collect();
    for (client, assoc) in world.stations(n) {
        let iface = assoc.band.ap_iface();
        if let Some(Value::Array(list)) = stations.get_mut(iface) {
            list.push(station(client, assoc, now_ms));
        }
        if let Some(Value::Object(map)) = clients.get_mut(iface) {
            map.insert(client.mac.to_string(), hostapd_client(client));
        }
    }
    if let Some(Value::Array(list)) = stations.get_mut("phy1-mesh0") {
        list.extend(mesh_peers(world, n));
    }
    json!({"stations": stations, "clients": clients})
}

fn station(client: &Client, assoc: &Assoc, now_ms: i64) -> Value {
    let signal = assoc.signal.round() as i64;
    let c = assoc.counters;
    json!({
        "mac": client.mac.to_string(),
        "info": {
            "inactive_time": assoc.inactive_ms,
            "connected_time": ((now_ms - assoc.since_ms) / 1000).max(0),
            "signal": signal,
            "signal_avg": signal + 1,
            "rx_bitrate": bitrate(assoc.rx_rate),
            "tx_bitrate": bitrate(assoc.tx_rate),
            "rx_bytes": c.rx_bytes & 0xffff_ffff,
            "tx_bytes": c.tx_bytes & 0xffff_ffff,
            "rx_bytes64": c.rx_bytes,
            "tx_bytes64": c.tx_bytes,
            "rx_packets": c.rx_packets,
            "tx_packets": c.tx_packets,
            "tx_retries": assoc.retries,
            "tx_failed": assoc.failed,
            "expected_throughput": assoc.tx_rate.expected_kbps(),
            "sta_flags": [254, 190],
            "rx_duration": c.rx_bytes / 40,
            "tx_duration": c.tx_bytes / 40,
        },
    })
}

fn bitrate(rate: Rate) -> Value {
    let mut value =
        json!({"bitrate": rate.bitrate.min(u32::from(u16::MAX)), "bitrate32": rate.bitrate});
    match rate.standard {
        Standard::He => {
            value["he_mcs"] = json!(rate.mcs);
            value["he_nss"] = json!(rate.streams);
            value["he_gi"] = json!(0);
        }
        Standard::Vht => {
            value["vht_mcs"] = json!(rate.mcs);
            value["vht_nss"] = json!(rate.streams);
        }
        Standard::Ht => value["mcs"] = json!(rate.mcs + 8 * (rate.streams - 1)),
    }
    match rate.width_mhz {
        80 => value["width_80"] = json!(true),
        40 => value["width_40"] = json!(true),
        _ => {}
    }
    value
}

fn hostapd_client(client: &Client) -> Value {
    let spec = client.spec;
    json!({
        "authorized": true,
        "wmm": true,
        "ht": true,
        "vht": spec.standard != Standard::Ht && spec.five_ghz,
        "he": spec.standard == Standard::He,
        "mfp": spec.sae,
        // Beacon reports (11k) and BSS transition (11v) come with the phones and laptops.
        "rrm": if spec.ft || spec.sae { json!([115, 0, 0, 0, 0]) } else { json!([0, 0, 0, 0, 0]) },
        "extended_capabilities": if spec.sae { json!([4, 0, 8, 2, 1, 64, 0, 64]) } else { json!([4, 0, 0, 0]) },
    })
}

/// The 802.11s peers the mesh interface has a link with.
fn mesh_peers(world: &World, n: usize) -> Vec<Value> {
    let uptime_s = (world.now_ms() - world.nodes[n].booted_ms) / 1000;
    (0..world.nodes.len())
        .filter(|&peer| peer != n && world.nodes[peer].up())
        .filter_map(|peer| {
            let signal = world.mesh_signal(n, peer);
            if signal < -88.0 {
                return None;
            }
            let noise = world.nodes[n].radios[1].noise;
            let rate = Rate::for_snr(signal - noise, Standard::He, 2, 80);
            let peer_up_s = (world.now_ms() - world.nodes[peer].booted_ms) / 1000;
            let mesh = world.nodes[n].radios[1].mesh;
            let share = world.nodes.len().max(2) as u64 - 1;
            Some(json!({
                "mac": world.nodes[peer].mesh_mac().to_string(),
                "info": {
                    "inactive_time": 10 + (world.now_s % 90),
                    "connected_time": uptime_s.min(peer_up_s).max(0),
                    "signal": signal.round() as i64,
                    "signal_avg": signal.round() as i64,
                    "rx_bitrate": bitrate(rate),
                    "tx_bitrate": bitrate(rate),
                    "rx_bytes64": mesh.rx_bytes / share,
                    "tx_bytes64": mesh.tx_bytes / share,
                    "rx_packets": mesh.rx_packets / share,
                    "tx_packets": mesh.tx_packets / share,
                    "tx_retries": mesh.tx_packets / share / 40,
                    "tx_failed": 0,
                    "expected_throughput": rate.expected_kbps(),
                    "sta_flags": [254, 186],
                    "plink_state": 4,
                },
            }))
        })
        .collect()
}
