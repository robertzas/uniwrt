//! Tailscale as the sim's gateway runs it (D-140): the package is there once its `firewall` file
//! holds the `tailscale` zone (an apply that turns Tailscale on installs it), and its daemon
//! starts logged out with tailscaled's default prefs, as a fresh install does. The `tailscale`
//! op ([`op`]) drives it: `set` takes the controller's prefs, a login with a key logs in at once
//! (a key with `invalid` in it is refused as tailscaled refuses one), a login without hands out a
//! URL and the demo's admin approves it [`APPROVED_MS`] later; `down`, `up` and `logout` do
//! what they say. Logged in, it runs in a demo tailnet with two peers, its routes approved.

use serde_json::{Value, json};

/// The package's version on 25.12.
const VERSION: &str = "1.104.1-1 (OpenWrt)";
/// The demo tailnet.
const TAILNET: &str = "demo@example.com";
const SUFFIX: &str = "tail5e1d.ts.net";
/// How long after an interactive login the demo's admin opens the URL.
pub(crate) const APPROVED_MS: i64 = 20_000;

/// The gateway's `tailscaled`.
#[derive(Debug, Clone, Default)]
pub(crate) struct Daemon {
    /// The prefs the controller set (`routes`, `exit_node`, `hostname`); `None`: the
    /// defaults, with the tailnet's DNS and routes taken and netfilter on.
    prefs: Option<Value>,
    logged_in: bool,
    want_running: bool,
    /// An interactive login's URL, and when the demo's admin opens it.
    login: Option<(String, i64)>,
}

impl Daemon {
    /// Logged in, once a waiting login's URL was opened by `now_ms`.
    fn logged_in(&self, now_ms: i64) -> bool {
        self.logged_in || self.login.as_ref().is_some_and(|(_, at)| *at <= now_ms)
    }

    fn state(&self, now_ms: i64) -> &'static str {
        match (self.logged_in(now_ms), self.want_running) {
            (true, true) => "Running",
            (true, false) => "Stopped",
            (false, _) => "NeedsLogin",
        }
    }
}

/// Whether the package is installed: the `firewall` file has the zone.
pub(crate) fn installed(firewall: Option<&uniwrt_uci::Package>) -> bool {
    firewall.is_some_and(|f| {
        f.sections_of("zone")
            .any(|s| s.option("name") == Some("tailscale"))
    })
}

/// The `tailscale` op's answer, or the agent's code and words.
pub(crate) fn op(
    daemon: &mut Daemon,
    installed: bool,
    now_ms: i64,
    args: &Value,
) -> Result<Value, (&'static str, String)> {
    if !installed {
        return Err(("not_found", "tailscale isn't installed".to_owned()));
    }
    if daemon.logged_in(now_ms) {
        daemon.logged_in = true;
        daemon.login = None;
    }
    let mut auth_url = None;
    match args["action"].as_str() {
        Some("set") => daemon.prefs = Some(args["prefs"].clone()),
        Some("up") => {
            daemon.prefs = Some(args["prefs"].clone());
            if !daemon.logged_in {
                return Err((
                    "failed",
                    "timeout waiting for Tailscale service to enter a Running state".to_owned(),
                ));
            }
            daemon.want_running = true;
        }
        Some("down") => daemon.want_running = false,
        Some("logout") => {
            daemon.logged_in = false;
            daemon.want_running = false;
            daemon.login = None;
        }
        Some("login") => {
            daemon.prefs = Some(args["prefs"].clone());
            if let Some(key) = args["auth_key"].as_str() {
                daemon.want_running = true;
                if key.contains("invalid") {
                    return Err((
                        "failed",
                        "backend error: invalid key: unable to validate API key".to_owned(),
                    ));
                }
                daemon.logged_in = true;
                daemon.login = None;
            } else if !daemon.logged_in {
                daemon.want_running = true;
                let (url, _) = daemon.login.get_or_insert_with(|| {
                    let url = format!("https://login.tailscale.com/a/sim{now_ms:x}");
                    (url, now_ms + APPROVED_MS)
                });
                auth_url = Some(url.clone());
            }
        }
        _ => {
            return Err((
                "bad_request",
                "action must be set, up, down, logout or login".to_owned(),
            ));
        }
    }
    Ok(json!({"state": daemon.state(now_ms), "auth_url": auth_url}))
}

/// The daemon's status and prefs, as the agent passes them on; `null` without the package.
/// `name` is the gateway's hostname.
pub(super) fn tailscale(
    firewall: Option<&uniwrt_uci::Package>,
    daemon: &Daemon,
    name: &str,
    now_ms: i64,
) -> Value {
    if !installed(firewall) {
        return Value::Null;
    }
    let state = daemon.state(now_ms);
    let set = daemon.prefs.as_ref();
    let mut routes: Vec<Value> = set
        .and_then(|p| p["routes"].as_array().cloned())
        .unwrap_or_default();
    if set.is_some_and(|p| p["exit_node"] == true) {
        routes.extend([json!("0.0.0.0/0"), json!("::/0")]);
    }
    let hostname = set.and_then(|p| p["hostname"].as_str()).unwrap_or_default();
    let prefs = json!({
        "want_running": daemon.want_running, "logged_out": !daemon.logged_in(now_ms),
        "routes": routes, "hostname": hostname, "accept_dns": set.is_none(),
        "accept_routes": set.is_none(), "netfilter": if set.is_some() { 0 } else { 2 },
    });
    if state != "Running" {
        let health = if daemon.want_running {
            "You are logged out."
        } else {
            "Tailscale is stopped."
        };
        return json!({
            "state": state, "version": VERSION, "tailnet": null, "dns_suffix": "",
            "self": {"dns_name": "", "ips": null, "online": false},
            "health": [health], "peer_count": 0, "peers": [], "prefs": prefs,
        });
    }
    let machine = if hostname.is_empty() { name } else { hostname };
    let own = ["100.88.12.1", "fd7a:115c:a1e0::5801:c01"];
    let mut allowed: Vec<Value> = vec![
        json!("100.88.12.1/32"),
        json!("fd7a:115c:a1e0::5801:c01/128"),
    ];
    allowed.extend(routes.iter().cloned());
    let primary: Vec<&Value> = routes
        .iter()
        .filter(|r| !r.as_str().is_some_and(|r| r.ends_with("/0")))
        .collect();
    json!({
        "state": state, "version": VERSION, "tailnet": TAILNET, "dns_suffix": SUFFIX,
        "self": {
            "dns_name": format!("{machine}.{SUFFIX}."), "ips": own, "online": true,
            "allowed_ips": allowed, "primary_routes": primary,
        },
        "health": [], "peer_count": 2,
        "peers": [
            {"name": "laptop", "dns_name": format!("laptop.{SUFFIX}."), "os": "macOS",
             "ips": ["100.88.12.7"], "online": true, "active": true, "exit_node_option": false,
             "cur_addr": "198.51.100.23:41641", "relay": "fra",
             "last_seen": "0001-01-01T00:00:00Z", "rx": 18_432_771, "tx": 2_204_118},
            {"name": "phone", "dns_name": format!("phone.{SUFFIX}."), "os": "iOS",
             "ips": ["100.88.12.9"], "online": false, "active": false, "exit_node_option": false,
             "cur_addr": "", "relay": "fra", "last_seen": "2026-11-19T07:42:10Z",
             "rx": 0, "tx": 0},
        ],
        "prefs": prefs,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use serde_json::json;

    use super::{APPROVED_MS, Daemon, op, tailscale};

    fn zone() -> uniwrt_uci::Package {
        uniwrt_uci::parse(
            "firewall",
            "config zone\n\toption name 'tailscale'\n\tlist device 'tailscale0'\n",
        )
        .unwrap()
    }

    #[test]
    fn the_zone_brings_a_daemon_waiting_for_its_login() {
        let daemon = Daemon::default();
        let firewall = zone();
        let status = tailscale(Some(&firewall), &daemon, "gw", 0);
        assert_eq!(status["state"], "NeedsLogin");
        assert_eq!(status["prefs"]["netfilter"], 2);
        let lan = uniwrt_uci::parse("firewall", "config zone\n\toption name 'lan'\n").unwrap();
        assert_eq!(
            tailscale(Some(&lan), &daemon, "gw", 0),
            serde_json::Value::Null
        );
        assert_eq!(tailscale(None, &daemon, "gw", 0), serde_json::Value::Null);
        assert_eq!(
            op(&mut Daemon::default(), false, 0, &json!({"action": "down"}))
                .unwrap_err()
                .0,
            "not_found"
        );
    }

    #[test]
    fn an_interactive_login_is_approved_later_and_runs_with_the_prefs() {
        let firewall = zone();
        let mut daemon = Daemon::default();
        let prefs = json!({"routes": ["10.0.0.0/24"], "exit_node": true, "hostname": ""});
        op(
            &mut daemon,
            true,
            0,
            &json!({"action": "set", "prefs": prefs}),
        )
        .unwrap();
        let first = op(
            &mut daemon,
            true,
            1_000,
            &json!({"action": "login", "prefs": prefs}),
        )
        .unwrap();
        let url = first["auth_url"].as_str().unwrap().to_owned();
        assert_eq!(first["state"], "NeedsLogin");
        // Asked again while it waits: the same URL.
        let again = op(
            &mut daemon,
            true,
            2_000,
            &json!({"action": "login", "prefs": prefs}),
        )
        .unwrap();
        assert_eq!(again["auth_url"], url.as_str());
        let status = tailscale(Some(&firewall), &daemon, "gw", 1_000 + APPROVED_MS);
        assert_eq!(status["state"], "Running");
        assert_eq!(
            status["prefs"]["routes"],
            json!(["10.0.0.0/24", "0.0.0.0/0", "::/0"])
        );
        assert_eq!(status["self"]["primary_routes"], json!(["10.0.0.0/24"]));
        assert_eq!(status["self"]["dns_name"], "gw.tail5e1d.ts.net.");
        // Down, up, logout.
        let now = 2_000 + APPROVED_MS;
        assert_eq!(
            op(&mut daemon, true, now, &json!({"action": "down"})).unwrap()["state"],
            "Stopped"
        );
        assert_eq!(
            op(
                &mut daemon,
                true,
                now,
                &json!({"action": "up", "prefs": prefs})
            )
            .unwrap()["state"],
            "Running"
        );
        assert_eq!(
            op(&mut daemon, true, now, &json!({"action": "logout"})).unwrap()["state"],
            "NeedsLogin"
        );
    }

    #[test]
    fn a_key_logs_in_at_once_unless_it_is_refused() {
        let mut daemon = Daemon::default();
        let prefs = json!({"routes": [], "exit_node": false, "hostname": "home"});
        let bad = json!({"action": "login", "prefs": prefs, "auth_key": "tskey-invalid-1"});
        assert_eq!(op(&mut daemon, true, 0, &bad).unwrap_err().0, "failed");
        let good = json!({"action": "login", "prefs": prefs, "auth_key": "tskey-auth-1"});
        let answer = op(&mut daemon, true, 0, &good).unwrap();
        assert_eq!(
            (answer["state"].as_str(), answer["auth_url"].as_str()),
            (Some("Running"), None)
        );
    }
}
