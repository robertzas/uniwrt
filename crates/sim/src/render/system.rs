//! The sets `board`, `system` and `capabilities`, shaped as `node-agent/collect/` sends them:
//! ubus `system board` and `system info`, `/etc/openwrt_release`, board.json, thermal zones.

use serde_json::{Value, json};

use crate::world::{Node, World};

pub(super) fn board(node: &Node) -> Value {
    let (model, release) = (node.model, node.release);
    let description = format!("OpenWrt {} {}", release.version, release.revision);
    let mut network = json!({
        "lan": {"ports": model.lan_ports, "protocol": "static"},
        "wan": {"device": model.wan_port, "protocol": "dhcp"},
    });
    if node.gateway {
        network["lan"]["ports"] = json!(["lan1", "lan2", "lan3", "lan4", "lan5"]);
    }
    json!({
        "board": {
            "kernel": release.kernel,
            "hostname": node.name,
            "system": model.cpu,
            "model": model.name,
            "board_name": model.board_name,
            "rootfs_type": "squashfs",
            "release": {
                "distribution": "OpenWrt",
                "version": release.version,
                "firmware_url": "https://downloads.openwrt.org/",
                "revision": release.revision,
                "target": model.target,
                "description": description,
                "builddate": release.build_date,
            },
        },
        "release": {
            "DISTRIB_ID": "OpenWrt",
            "DISTRIB_RELEASE": release.version,
            "DISTRIB_REVISION": release.revision,
            "DISTRIB_TARGET": model.target,
            "DISTRIB_ARCH": model.arch,
            "DISTRIB_DESCRIPTION": description,
            "DISTRIB_TAINTS": "",
        },
        "board_json": {
            "model": {"id": model.board_name, "name": model.name},
            "system": {"label_macaddr": node.mac.to_string()},
            "network": network,
        },
    })
}

pub(super) fn system(world: &World, node: &Node) -> Value {
    let now_ms = world.now_ms();
    let memory = node.model.memory;
    let used = (memory as f64 * node.memory_used) as u64;
    let spare = memory - used;
    let overlay = node.model.overlay_kib;
    let overlay_used = (overlay as f64 * if node.gateway { 0.04 } else { 0.21 }) as u64;
    let tmp = memory / 2 / 1024;
    let tmp_used = 900 + node.logd.records.len() as u64;
    let fs = |total: u64, used: u64| json!({"total": total, "free": total - used, "used": used, "avail": total - used});
    // `system info` gives loads in 1/65536.
    let load = |factor: f64| ((node.load * factor) * 65_536.0) as u64;
    json!({
        "info": {
            "localtime": now_ms / 1000,
            "uptime": (now_ms - node.booted_ms) / 1000,
            "load": [load(1.0), load(0.9), load(0.8)],
            "memory": {
                "total": memory,
                "free": spare * 2 / 5,
                "shared": tmp_used * 1024,
                "buffered": spare / 40,
                "available": spare,
                "cached": spare / 2,
            },
            "root": fs(overlay, overlay_used),
            "tmp": fs(tmp, tmp_used),
            "swap": {"total": 0, "free": 0},
        },
        "clock": [now_ms / 1000, (now_ms % 1000) * 1_000_000],
        "cpu_percent": (node.cpu * 10.0).round() / 10.0,
        "thermal": [{"type": "cpu-thermal", "temp": (node.temp * 1000.0) as i64}],
    })
}

pub(super) fn capabilities(node: &Node) -> Value {
    let mut software = vec!["batman-adv", "batctl", "iw", "rpcd-mod-iwinfo", "luci"];
    if node.gateway {
        software.extend(["nlbwmon", "iperf3"]);
    }
    json!({
        "package_managers": [node.release.package_manager],
        "software": software,
        "hostapd": ["eap", "11r", "acs", "sae", "owe", "wps"],
        "wpa_supplicant": ["eap", "sae", "owe", "mesh"],
        "swconfig": false,
        "dsa": node.gateway,
    })
}
