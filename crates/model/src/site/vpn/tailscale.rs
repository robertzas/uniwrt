//! Tailscale on the gateway (features/network.md, VPN; D-140): the `tailscale` package joins
//! the gateway to a tailnet as a subnet router for the site's chosen networks and, where asked,
//! as an exit node. Schema v18 adds it.
//!
//! - **Login:** through the login URL the gateway's `tailscaled` hands out, relayed to the UI,
//!   or by itself with an auth key sealed in the vault ([`Tailscale::auth_key`]).
//! - **Firewall:** `tailscale0` is a zone of its own, [`ZONE`], with its own row in the zone
//!   matrix (seeded like the default LAN's), independent of WireGuard's `vpn`.
//! - **Gateway only:** the package is ~24 MB installed on 24.10 and ~29 MB on 25.12, so APs never
//!   run it.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{
    facts::TailscalePrefs,
    site::{SecretRef, SiteConfig},
};

/// The firewall zone of `tailscale0`.
pub const ZONE: &str = "tailscale";

/// The device `tailscaled` makes.
pub const DEVICE: &str = "tailscale0";

/// The package's UDP port (its `settings.port`), which the WAN opens for direct connections.
pub const PORT: u16 = 41641;

/// Where the auth key sits in the document.
pub const AUTH_KEY: &str = "vpn/tailscale/auth_key";

/// The gateway's Tailscale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Tailscale {
    pub enabled: bool,
    /// The site's networks the gateway advertises to the tailnet as subnet routes, by id.
    pub routes: Vec<String>,
    /// Offers the gateway to the tailnet as an exit node.
    pub exit_node: bool,
    /// Its machine name in the tailnet; the gateway's hostname where none.
    pub hostname: Option<String>,
    /// An auth key the gateway logs in with by itself; without one, someone opens the login URL.
    pub auth_key: Option<SecretRef>,
}

impl Tailscale {
    /// Tailscale on, advertising `routes`, logged in through the URL.
    pub fn new(routes: Vec<String>) -> Self {
        Self {
            enabled: true,
            routes,
            exit_node: false,
            hostname: None,
            auth_key: None,
        }
    }
}

/// The prefs the gateway's `tailscaled` should hold (the `tailscale` op's `prefs`; D-140). The
/// agent adds what never changes: the tailnet's DNS and routes refused, the firewall left to fw4.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Prefs {
    /// The advertised networks' IPv4 subnets, such as `10.0.30.0/24`, in the document's order.
    pub routes: Vec<String>,
    pub exit_node: bool,
    /// The machine name; empty for the gateway's hostname.
    pub hostname: String,
}

impl Tailscale {
    /// Its prefs in `config`: each advertised network's subnet (a network without one, which
    /// validation refuses, is left out).
    pub fn prefs(&self, config: &SiteConfig) -> Prefs {
        let routes = self
            .routes
            .iter()
            .filter_map(|id| config.networks.iter().find(|n| n.id == *id))
            .filter_map(|network| network.ipv4.as_ref())
            .map(|ipv4| ipv4.address.trunc().to_string())
            .collect();
        Prefs {
            routes,
            exit_node: self.exit_node,
            hostname: self.hostname.clone().unwrap_or_default(),
        }
    }
}

impl Prefs {
    /// Whether the daemon holds these (`held`), with the tailnet's DNS and routes refused and the
    /// firewall left to fw4. An exit node advertises `0.0.0.0/0` and `::/0` beside its routes.
    pub fn held_by(&self, held: &TailscalePrefs) -> bool {
        let mut want: BTreeSet<&str> = self.routes.iter().map(String::as_str).collect();
        if self.exit_node {
            want.extend(["0.0.0.0/0", "::/0"]);
        }
        let have: BTreeSet<&str> = held.advertise_routes.iter().map(String::as_str).collect();
        want == have
            && held.hostname.as_deref().unwrap_or_default() == self.hostname
            && !held.accept_dns
            && !held.accept_routes
            && held.netfilter_off
    }
}

/// Whether `key` looks like a Tailscale auth key: `tskey-` and up to 250 more letters, digits,
/// `-` or `_`.
pub fn is_auth_key(key: &str) -> bool {
    key.strip_prefix("tskey-").is_some_and(|rest| {
        !rest.is_empty()
            && rest.len() <= 250
            && rest
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    })
}

#[cfg(test)]
mod tests {
    use super::{Prefs, is_auth_key};
    use crate::facts::TailscalePrefs;

    #[test]
    fn prefs_are_held_whatever_the_order_with_the_exit_node_s_routes() {
        let want = Prefs {
            routes: vec!["10.0.30.0/24".into(), "10.0.0.0/24".into()],
            exit_node: true,
            hostname: "home".into(),
        };
        let mut held = TailscalePrefs {
            want_running: true,
            logged_out: false,
            advertise_routes: ["0.0.0.0/0", "::/0", "10.0.0.0/24", "10.0.30.0/24"]
                .map(String::from)
                .to_vec(),
            hostname: Some("home".into()),
            accept_dns: false,
            accept_routes: false,
            netfilter_off: true,
        };
        assert!(want.held_by(&held));
        let no_exit = Prefs {
            exit_node: false,
            ..want.clone()
        };
        assert!(!no_exit.held_by(&held));
        held.hostname = None;
        assert!(!want.held_by(&held));
        held.hostname = Some("home".into());
        held.netfilter_off = false;
        assert!(!want.held_by(&held));
        held.netfilter_off = true;
        held.accept_dns = true;
        assert!(!want.held_by(&held));
    }

    #[test]
    fn auth_keys_start_with_tskey() {
        assert!(is_auth_key("tskey-auth-kBc1CNTRL-abcdefghijklmnop"));
        assert!(!is_auth_key("tskey-"));
        assert!(!is_auth_key("auth-kBc1CNTRL-abc"));
        assert!(!is_auth_key("tskey-auth abc"));
        assert!(!is_auth_key(&format!("tskey-{}", "a".repeat(251))));
    }
}
