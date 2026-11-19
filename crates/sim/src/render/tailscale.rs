//! Tailscale as the sim's gateway reports it in its `services` set (D-140): the package is there
//! once its `firewall` file holds the `tailscale` zone (an apply that turns Tailscale on
//! installs it), and its daemon waits for a login, as a fresh install does.

use serde_json::{Value, json};

/// The package's version on 25.12.
const VERSION: &str = "1.104.1-1 (OpenWrt)";

/// The daemon's status, as the agent passes it on; `null` without the package.
pub(super) fn tailscale(firewall: Option<&uniwrt_uci::Package>) -> Value {
    let zone = firewall.and_then(|f| {
        f.sections_of("zone")
            .find(|s| s.option("name") == Some("tailscale"))
    });
    if zone.is_none() {
        return Value::Null;
    }
    json!({
        "state": "NeedsLogin", "version": VERSION, "tailnet": null, "dns_suffix": "",
        "self": {"dns_name": "", "ips": null, "online": false},
        "health": ["Tailscale is stopped."], "peer_count": 0, "peers": [],
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::tailscale;

    #[test]
    fn the_zone_brings_a_daemon_waiting_for_its_login() {
        let firewall = uniwrt_uci::parse(
            "firewall",
            "config zone\n\toption name 'tailscale'\n\tlist device 'tailscale0'\n",
        )
        .unwrap();
        assert_eq!(tailscale(Some(&firewall))["state"], "NeedsLogin");
        let lan = uniwrt_uci::parse("firewall", "config zone\n\toption name 'lan'\n").unwrap();
        assert_eq!(tailscale(Some(&lan)), serde_json::Value::Null);
        assert_eq!(tailscale(None), serde_json::Value::Null);
    }
}
