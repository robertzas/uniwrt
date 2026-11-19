//! Tailscale's part of the `services` set (D-140): the fields of `tailscale status --json` the
//! agent passes on (never the login URL), typed. Tailscale writes empty strings, zero times
//! (`0001-01-01T00:00:00Z`) and `null` lists for what it doesn't know: they become `None` and
//! empty lists.

use std::net::IpAddr;

use serde::Deserialize;
use uniwrt_core::Timestamp;
use uniwrt_model::facts::{TailscaleFacts, TailscalePeerFacts, TailscalePrefs, TailscaleState};

#[derive(Deserialize)]
pub(super) struct Raw {
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    tailnet: Option<String>,
    #[serde(default)]
    dns_suffix: Option<String>,
    #[serde(default, rename = "self")]
    me: Option<Me>,
    #[serde(default)]
    health: Option<Vec<String>>,
    #[serde(default)]
    peer_count: Option<u32>,
    #[serde(default)]
    peers: Option<Vec<Peer>>,
    #[serde(default)]
    prefs: Option<Prefs>,
}

/// `tailscale debug prefs`'s fields the agent passes on.
#[derive(Deserialize)]
struct Prefs {
    #[serde(default)]
    want_running: Option<bool>,
    #[serde(default)]
    logged_out: Option<bool>,
    #[serde(default)]
    routes: Option<Vec<String>>,
    #[serde(default)]
    hostname: Option<String>,
    #[serde(default)]
    accept_dns: Option<bool>,
    #[serde(default)]
    accept_routes: Option<bool>,
    /// 0 off, 1 `nodivert`, 2 on.
    #[serde(default)]
    netfilter: Option<i64>,
}

#[derive(Deserialize)]
struct Me {
    #[serde(default)]
    dns_name: Option<String>,
    #[serde(default)]
    ips: Option<Vec<String>>,
    #[serde(default)]
    online: Option<bool>,
    #[serde(default)]
    allowed_ips: Option<Vec<String>>,
    #[serde(default)]
    primary_routes: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct Peer {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    dns_name: Option<String>,
    #[serde(default)]
    os: Option<String>,
    #[serde(default)]
    ips: Option<Vec<String>>,
    #[serde(default)]
    online: Option<bool>,
    #[serde(default)]
    active: Option<bool>,
    #[serde(default)]
    exit_node_option: Option<bool>,
    #[serde(default)]
    cur_addr: Option<String>,
    #[serde(default)]
    relay: Option<String>,
    #[serde(default)]
    last_seen: Option<String>,
    #[serde(default)]
    rx: Option<u64>,
    #[serde(default)]
    tx: Option<u64>,
}

pub(super) fn parse(raw: Raw) -> TailscaleFacts {
    let me = raw.me.unwrap_or(Me {
        dns_name: None,
        ips: None,
        online: None,
        allowed_ips: None,
        primary_routes: None,
    });
    let addresses = addresses(me.ips);
    // AllowedIPs holds the machine's own addresses as /32 and /128 next to its approved routes.
    let own: Vec<String> = addresses
        .iter()
        .map(|ip| match ip {
            IpAddr::V4(v4) => format!("{v4}/32"),
            IpAddr::V6(v6) => format!("{v6}/128"),
        })
        .collect();
    let mut approved_routes: Vec<String> = Vec::new();
    for route in me
        .allowed_ips
        .into_iter()
        .flatten()
        .chain(me.primary_routes.into_iter().flatten())
    {
        if !own.contains(&route) && !approved_routes.contains(&route) {
            approved_routes.push(route);
        }
    }
    let mut peers: Vec<TailscalePeerFacts> = raw
        .peers
        .unwrap_or_default()
        .into_iter()
        .map(peer)
        .collect();
    peers.sort_by(|a, b| b.online.cmp(&a.online).then_with(|| a.name.cmp(&b.name)));
    peers.truncate(uniwrt_model::facts::TAILSCALE_MAX_PEERS);
    TailscaleFacts {
        state: raw.state.as_deref().map(TailscaleState::from_backend),
        version: text(raw.version),
        tailnet: text(raw.tailnet),
        dns_suffix: text(raw.dns_suffix),
        dns_name: dns_name(me.dns_name),
        addresses,
        online: me.online.unwrap_or_default(),
        approved_routes,
        health: raw.health.unwrap_or_default(),
        peer_count: raw
            .peer_count
            .unwrap_or_default()
            .max(u32::try_from(peers.len()).unwrap_or(u32::MAX)),
        peers,
        prefs: raw.prefs.map(prefs),
    }
}

fn prefs(raw: Prefs) -> TailscalePrefs {
    TailscalePrefs {
        want_running: raw.want_running.unwrap_or_default(),
        logged_out: raw.logged_out.unwrap_or_default(),
        advertise_routes: raw.routes.unwrap_or_default(),
        hostname: text(raw.hostname),
        accept_dns: raw.accept_dns.unwrap_or_default(),
        accept_routes: raw.accept_routes.unwrap_or_default(),
        netfilter_off: raw.netfilter == Some(0),
    }
}

fn peer(raw: Peer) -> TailscalePeerFacts {
    TailscalePeerFacts {
        name: raw.name.unwrap_or_default(),
        dns_name: dns_name(raw.dns_name),
        os: text(raw.os),
        addresses: addresses(raw.ips),
        online: raw.online.unwrap_or_default(),
        active: raw.active.unwrap_or_default(),
        exit_node_option: raw.exit_node_option.unwrap_or_default(),
        direct: text(raw.cur_addr),
        relay: text(raw.relay),
        // Tailscale's zero time is year 1.
        last_seen: raw
            .last_seen
            .as_deref()
            .and_then(Timestamp::parse_utc)
            .filter(|at| at.as_unix_ms() > 0),
        rx_bytes: raw.rx.unwrap_or_default(),
        tx_bytes: raw.tx.unwrap_or_default(),
    }
}

fn text(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.is_empty())
}

fn dns_name(value: Option<String>) -> Option<String> {
    text(value.map(|name| name.trim_end_matches('.').to_owned()))
}

fn addresses(ips: Option<Vec<String>>) -> Vec<IpAddr> {
    ips.unwrap_or_default()
        .iter()
        .filter_map(|ip| ip.parse().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use uniwrt_core::Timestamp;
    use uniwrt_model::facts::TailscaleState;

    use super::parse;

    #[test]
    fn a_logged_out_daemon_reports_its_state_alone() {
        // As 24.10.8's and 25.12.6's packages answer before a login (the agent's fields).
        let raw = serde_json::json!({
            "state": "NeedsLogin", "version": "1.104.1-1 (OpenWrt)", "tailnet": null,
            "dns_suffix": "", "health": ["Tailscale is stopped."], "peer_count": 0, "peers": [],
            "self": {"dns_name": "", "ips": null, "online": false}
        });
        let facts = parse(serde_json::from_value(raw).unwrap());
        assert_eq!(facts.state, Some(TailscaleState::NeedsLogin));
        assert_eq!(facts.version.as_deref(), Some("1.104.1-1 (OpenWrt)"));
        assert_eq!(
            (facts.tailnet, facts.dns_suffix, facts.dns_name),
            (None, None, None)
        );
        assert!(facts.addresses.is_empty() && facts.approved_routes.is_empty());
        assert_eq!(facts.health, ["Tailscale is stopped."]);
    }

    #[test]
    fn a_running_gateway_reports_its_routes_and_peers() {
        let raw = serde_json::json!({
            "state": "Running", "version": "1.104.1-1 (OpenWrt)", "tailnet": "robin@example.com",
            "dns_suffix": "tail1234.ts.net", "health": [], "peer_count": 3,
            "self": {
                "dns_name": "home-gateway.tail1234.ts.net.",
                "ips": ["100.101.102.103", "fd7a:115c:a1e0::1"],
                "online": true,
                "allowed_ips": ["100.101.102.103/32", "fd7a:115c:a1e0::1/128", "10.0.0.0/24",
                                "0.0.0.0/0", "::/0"],
                "primary_routes": ["10.0.0.0/24"]
            },
            "peers": [
                {"name": "laptop", "dns_name": "laptop.tail1234.ts.net.", "os": "macOS",
                 "ips": ["100.64.1.2"], "online": false, "active": false,
                 "exit_node_option": false, "cur_addr": "", "relay": "fra",
                 "last_seen": "2026-11-19T15:00:00.5Z", "rx": 0, "tx": 0},
                {"name": "phone", "dns_name": "phone.tail1234.ts.net.", "os": "iOS",
                 "ips": ["100.64.1.3"], "online": true, "active": true,
                 "exit_node_option": false, "cur_addr": "198.51.100.7:41641", "relay": "fra",
                 "last_seen": "0001-01-01T00:00:00Z", "rx": 1234, "tx": 5678},
                {"name": "pi", "online": true}
            ]
        });
        let facts = parse(serde_json::from_value(raw).unwrap());
        assert_eq!(facts.state, Some(TailscaleState::Running));
        assert_eq!(
            facts.dns_name.as_deref(),
            Some("home-gateway.tail1234.ts.net")
        );
        assert_eq!(facts.addresses.len(), 2);
        assert_eq!(facts.approved_routes, ["10.0.0.0/24", "0.0.0.0/0", "::/0"]);
        let names: Vec<&str> = facts.peers.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["phone", "pi", "laptop"]);
        let phone = &facts.peers[0];
        assert_eq!(phone.direct.as_deref(), Some("198.51.100.7:41641"));
        assert_eq!(
            (phone.last_seen, phone.rx_bytes, phone.tx_bytes),
            (None, 1234, 5678)
        );
        let laptop = &facts.peers[2];
        assert_eq!(laptop.direct, None);
        assert_eq!(
            laptop.last_seen,
            Timestamp::parse_utc("2026-11-19T15:00:00.5Z")
        );
        assert_eq!(facts.peer_count, 3);
    }

    #[test]
    fn prefs_are_read_as_tailscale_debug_prefs_has_them() {
        // As 25.12.6's daemon answers after `tailscale set` with an exit node (the agent's fields).
        let raw = serde_json::json!({"state": "NeedsLogin", "prefs": {
            "want_running": false, "logged_out": true, "hostname": "gw-test",
            "routes": ["0.0.0.0/0", "::/0", "192.168.10.0/24"], "accept_dns": false,
            "accept_routes": false, "netfilter": 0
        }});
        let prefs = parse(serde_json::from_value(raw).unwrap()).prefs.unwrap();
        assert!(!prefs.want_running && prefs.logged_out);
        assert_eq!(
            prefs.advertise_routes,
            ["0.0.0.0/0", "::/0", "192.168.10.0/24"]
        );
        assert_eq!(prefs.hostname.as_deref(), Some("gw-test"));
        assert!(!prefs.accept_dns && !prefs.accept_routes && prefs.netfilter_off);
        // A fresh install: tailscaled's defaults (DNS and routes taken, netfilter on), no name.
        let raw = serde_json::json!({"state": "NeedsLogin", "prefs": {
            "want_running": false, "logged_out": true, "hostname": "", "routes": null,
            "accept_dns": true, "accept_routes": true, "netfilter": 2
        }});
        let prefs = parse(serde_json::from_value(raw).unwrap()).prefs.unwrap();
        assert_eq!(prefs.hostname, None);
        assert!(prefs.advertise_routes.is_empty() && !prefs.netfilter_off);
    }

    #[test]
    fn an_unknown_state_and_a_silent_daemon_are_kept_apart() {
        let facts = parse(serde_json::from_value(serde_json::json!({"state": "Weird"})).unwrap());
        assert_eq!(facts.state, Some(TailscaleState::Unknown));
        let facts = parse(serde_json::from_value(serde_json::json!({"state": null})).unwrap());
        assert_eq!(facts.state, None);
    }
}
