//! The `services` set: ddns-scripts' run files per section (the registered address, the last
//! update in seconds of uptime, whether the updater runs), https-dns-proxy's and stubby's
//! running instances (D-146), with the node's clock and uptime, which place the update in time; and the DNS
//! blocklist's header line and conf file (D-133); and the WireGuard tunnel's peers and
//! [`Tailscale`](super::tailscale) (D-140).

use std::net::{IpAddr, SocketAddr};

use serde::Deserialize;
use uniwrt_core::Timestamp;
use uniwrt_model::{
    blocklists::parse_header,
    facts::{BlocklistFacts, DdnsFacts, ServiceFacts, WireguardFacts, WireguardPeerFacts},
};

#[derive(Deserialize)]
pub(super) struct Raw {
    #[serde(default)]
    now: Option<i64>,
    #[serde(default)]
    uptime: Option<i64>,
    #[serde(default)]
    ddns: Vec<Ddns>,
    #[serde(default)]
    doh: Option<u32>,
    #[serde(default)]
    dot: Option<u32>,
    /// The blocklist's first line.
    #[serde(default)]
    blocklist: Option<String>,
    #[serde(default)]
    blocklist_conf: bool,
    #[serde(default)]
    wireguard: Option<Wireguard>,
    #[serde(default)]
    tailscale: Option<super::tailscale::Raw>,
}

/// `wg show vpn dump`'s fields, as text.
#[derive(Deserialize)]
struct Wireguard {
    #[serde(default)]
    public_key: Option<String>,
    #[serde(default)]
    port: Option<String>,
    #[serde(default)]
    peers: Vec<WireguardPeer>,
}

#[derive(Deserialize)]
struct WireguardPeer {
    public_key: String,
    #[serde(default)]
    endpoint: Option<String>,
    #[serde(default)]
    handshake: Option<String>,
    #[serde(default)]
    rx: Option<String>,
    #[serde(default)]
    tx: Option<String>,
}

#[derive(Deserialize)]
struct Ddns {
    id: String,
    #[serde(default)]
    ip: Option<String>,
    #[serde(default)]
    update: Option<String>,
    #[serde(default)]
    running: bool,
}

pub(super) fn parse(raw: Raw) -> ServiceFacts {
    let ddns = raw
        .ddns
        .into_iter()
        .map(|d| {
            // `.update` is 0 until a first update; it counts from boot.
            let at = d
                .update
                .and_then(|u| u.trim().parse::<i64>().ok())
                .filter(|&u| u > 0);
            let updated_at = match (raw.now, raw.uptime, at) {
                (Some(now), Some(uptime), Some(at)) if at <= uptime => {
                    Some(Timestamp::from_unix_secs(now - uptime + at))
                }
                _ => None,
            };
            DdnsFacts {
                id: d.id,
                address: d.ip.and_then(|ip| ip.trim().parse::<IpAddr>().ok()),
                updated_at,
                running: d.running,
            }
        })
        .collect();
    let header = raw.blocklist.as_deref().and_then(parse_header);
    let (sha256, domains) = header.map_or((None, None), |(sha, n)| (Some(sha), Some(n)));
    ServiceFacts {
        ddns,
        doh_instances: raw.doh,
        dot_instances: raw.dot,
        blocklist: BlocklistFacts {
            sha256,
            domains,
            conf: raw.blocklist_conf,
        },
        wireguard: raw.wireguard.map(|wg| wireguard(wg, raw.now)),
        tailscale: raw.tailscale.map(|t| Box::new(super::tailscale::parse(t))),
    }
}

/// `(none)` stands for no endpoint and a handshake of 0 for none; a key that isn't one is
/// kept as it is (the API matches peers by it).
fn wireguard(raw: Wireguard, now: Option<i64>) -> WireguardFacts {
    let number = |text: Option<&str>| text.and_then(|t| t.trim().parse::<u64>().ok());
    let peers = raw
        .peers
        .into_iter()
        .map(|peer| WireguardPeerFacts {
            public_key: peer.public_key,
            endpoint: peer
                .endpoint
                .and_then(|e| e.trim().parse::<SocketAddr>().ok()),
            handshake_at: number(peer.handshake.as_deref())
                .filter(|&at| at > 0)
                .and_then(|at| i64::try_from(at).ok())
                .map(Timestamp::from_unix_secs),
            rx_bytes: number(peer.rx.as_deref()).unwrap_or_default(),
            tx_bytes: number(peer.tx.as_deref()).unwrap_or_default(),
        })
        .collect();
    WireguardFacts {
        node_time: now.map(Timestamp::from_unix_secs),
        public_key: raw.public_key.filter(|key| key != "(none)"),
        port: raw
            .port
            .and_then(|p| p.trim().parse().ok())
            .filter(|&p| p != 0),
        peers,
    }
}

#[cfg(test)]
mod tests {
    use uniwrt_core::Timestamp;

    use super::parse;

    #[test]
    fn updates_are_placed_on_the_clock_and_odd_files_skipped() {
        let raw = serde_json::from_str(
            r#"{"now": 1700000000, "uptime": 5000, "doh": 2, "dot": 1, "ddns": [
                {"id": "home", "ip": "203.0.113.7", "update": "4000", "running": true},
                {"id": "six", "ip": "2001:db8::1\n", "update": "0", "running": false},
                {"id": "new", "ip": null, "update": null, "running": false},
                {"id": "odd", "ip": "nonsense", "update": "9999", "running": false}
            ]}"#,
        )
        .unwrap();
        let facts = parse(raw);
        assert_eq!(
            (facts.doh_instances, facts.dot_instances),
            (Some(2), Some(1))
        );
        assert_eq!(
            facts.blocklist,
            uniwrt_model::facts::BlocklistFacts::default()
        );
        let home = &facts.ddns[0];
        assert_eq!(home.address, Some("203.0.113.7".parse().unwrap()));
        assert_eq!(
            home.updated_at,
            Some(Timestamp::from_unix_secs(1_699_999_000))
        );
        assert!(home.running);
        assert_eq!(facts.ddns[1].address, Some("2001:db8::1".parse().unwrap()));
        assert_eq!(facts.ddns[1].updated_at, None);
        assert_eq!(facts.ddns[2].address, None);
        // An update after the uptime is from before a reboot.
        assert_eq!(
            (facts.ddns[3].address, facts.ddns[3].updated_at),
            (None, None)
        );
    }

    #[test]
    fn the_tunnel_s_peers_are_read_from_the_dump() {
        let raw = serde_json::json!({
            "now": 1_700_000_000, "wireguard": {
                "public_key": "c2VydmVy", "port": "51820", "peers": [
                    {"public_key": "cGhvbmU=", "endpoint": "198.51.100.7:40000",
                     "handshake": "1699999950", "rx": "1234", "tx": "5678"},
                    {"public_key": "Y2FiaW4=", "endpoint": "[2001:db8::2]:51820",
                     "handshake": "0", "rx": "0", "tx": "92"},
                    {"public_key": "bmV3", "endpoint": "(none)", "handshake": "0",
                     "rx": "0", "tx": "0"}
                ]
            }
        });
        let wg = parse(serde_json::from_value(raw).unwrap())
            .wireguard
            .unwrap();
        assert_eq!(wg.node_time, Some(Timestamp::from_unix_secs(1_700_000_000)));
        assert_eq!(
            (wg.public_key.as_deref(), wg.port),
            (Some("c2VydmVy"), Some(51820))
        );
        let [phone, cabin, new] = &wg.peers[..] else {
            panic!("{:?}", wg.peers)
        };
        assert_eq!(phone.endpoint, Some("198.51.100.7:40000".parse().unwrap()));
        assert_eq!(
            phone.handshake_at,
            Some(Timestamp::from_unix_secs(1_699_999_950))
        );
        assert_eq!((phone.rx_bytes, phone.tx_bytes), (1234, 5678));
        assert_eq!(cabin.endpoint, Some("[2001:db8::2]:51820".parse().unwrap()));
        assert_eq!((cabin.handshake_at, cabin.tx_bytes), (None, 92));
        assert_eq!((new.endpoint, new.handshake_at), (None, None));
        // A gateway without the tunnel reports none.
        let facts = parse(serde_json::from_value(serde_json::json!({})).unwrap());
        assert_eq!(facts.wireguard, None);
    }

    #[test]
    fn the_blocklist_is_named_by_its_header() {
        let sha = "ab".repeat(32);
        let raw = serde_json::json!({
            "blocklist": format!("# uniwrt {sha} 1200"), "blocklist_conf": true,
        });
        let facts = parse(serde_json::from_value(raw).unwrap());
        assert_eq!(facts.blocklist.sha256, Some(sha));
        assert_eq!(facts.blocklist.domains, Some(1200));
        assert!(facts.blocklist.conf);
        // A file without the header (cut short, or someone else's) names nothing.
        let raw = serde_json::json!({"blocklist": "server=/a.com/", "blocklist_conf": false});
        let facts = parse(serde_json::from_value(raw).unwrap());
        assert_eq!(
            (facts.blocklist.sha256, facts.blocklist.conf),
            (None, false)
        );
    }
}
