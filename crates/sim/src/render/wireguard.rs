//! The WireGuard tunnel as the sim's gateway reports it in its `services` set (D-140): the
//! `vpn` interface of its `network` file and a peer per `wireguard_vpn` section, as
//! `wg show vpn dump` would give them. Each peer's traffic follows from its public key, so the
//! demo is the same on every run: a site peer is always up (it keeps the tunnel open), and two
//! client peers of three are connected, the third last seen three hours ago or never.

use serde_json::{Value, json};

/// How long ago a peer that isn't connected made its handshake, where it made one.
const AWAY_S: i64 = 3 * 3600;

/// The tunnel, `None` without its interface.
pub(super) fn wireguard(network: Option<&uniwrt_uci::Package>, now: i64, uptime: i64) -> Value {
    let Some(network) = network else {
        return Value::Null;
    };
    let tunnel = network
        .section("vpn")
        .filter(|s| s.kind == "interface" && s.option("proto") == Some("wireguard"));
    let Some(tunnel) = tunnel else {
        return Value::Null;
    };
    let peers: Vec<Value> = network
        .sections_of("wireguard_vpn")
        .filter(|s| s.option("disabled") != Some("1"))
        .filter_map(|s| {
            let key = s.option("public_key")?;
            let site = s.option("route_allowed_ips") == Some("1");
            Some(peer(
                key,
                site,
                s.option("endpoint_host"),
                s.option("endpoint_port"),
                now,
                uptime,
            ))
        })
        .collect();
    json!({
        "public_key": null,
        "port": tunnel.option("listen_port").unwrap_or("51820"),
        "peers": peers,
    })
}

fn peer(
    key: &str,
    site: bool,
    host: Option<&str>,
    port: Option<&str>,
    now: i64,
    uptime: i64,
) -> Value {
    let h = fnv(key);
    let connected = site || !h.is_multiple_of(3);
    let pick = |n: u64| i64::try_from(h % n).unwrap_or_default();
    let (handshake, active_s) = if connected {
        // WireGuard makes a handshake every two minutes while traffic flows.
        (now - pick(110).min(uptime) - 5, uptime)
    } else if h.is_multiple_of(2) && uptime > AWAY_S {
        (now - AWAY_S, uptime - AWAY_S)
    } else {
        (0, 0)
    };
    let rate = h % 40_000 + 2_000;
    let active = u64::try_from(active_s.max(0)).unwrap_or_default();
    let endpoint = match (host, port) {
        (Some(host), Some(port)) if host.parse::<std::net::IpAddr>().is_ok() => {
            if host.contains(':') {
                format!("[{host}]:{port}")
            } else {
                format!("{host}:{port}")
            }
        }
        _ if handshake > 0 => format!("198.51.100.{}:{}", h % 200 + 20, 30_000 + h % 30_000),
        _ => "(none)".to_owned(),
    };
    json!({
        "public_key": key,
        "endpoint": endpoint,
        "handshake": handshake.to_string(),
        "rx": (active * rate / 4).to_string(),
        "tx": (active * rate).to_string(),
    })
}

/// FNV-1a: a stable hash, unlike std's.
fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::wireguard;

    const NETWORK: &str = "config interface 'vpn'\n\toption proto 'wireguard'\n\toption listen_port '51821'\n\n\
        config wireguard_vpn 'wg_phone'\n\toption public_key 'cGhvbmU='\n\n\
        config wireguard_vpn 'wg_cabin'\n\toption public_key 'Y2FiaW4='\n\toption route_allowed_ips '1'\n\
        \toption endpoint_host '192.0.2.9'\n\toption endpoint_port '51820'\n";

    #[test]
    fn the_tunnel_s_peers_answer_as_wg_would() {
        let network = uniwrt_uci::parse("network", NETWORK).unwrap();
        let wg = wireguard(Some(&network), 1_700_000_000, 50_000);
        assert_eq!(wg["port"], "51821");
        let peers = wg["peers"].as_array().unwrap();
        assert_eq!(peers.len(), 2);
        let cabin = &peers[1];
        assert_eq!(cabin["endpoint"], "192.0.2.9:51820");
        let handshake: i64 = cabin["handshake"].as_str().unwrap().parse().unwrap();
        assert!(
            (1_700_000_000 - 120..1_700_000_000).contains(&handshake),
            "{handshake}"
        );
        assert_ne!(cabin["tx"], "0");
        // The same key, the same traffic.
        assert_eq!(wireguard(Some(&network), 1_700_000_000, 50_000), wg);
        assert_eq!(wireguard(None, 0, 0), serde_json::Value::Null);
        let lan = uniwrt_uci::parse("network", "config interface 'lan'\n").unwrap();
        assert_eq!(wireguard(Some(&lan), 0, 0), serde_json::Value::Null);
    }
}
