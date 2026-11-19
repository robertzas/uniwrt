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

use serde::{Deserialize, Serialize};

use crate::site::SecretRef;

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
    use super::is_auth_key;

    #[test]
    fn auth_keys_start_with_tskey() {
        assert!(is_auth_key("tskey-auth-kBc1CNTRL-abcdefghijklmnop"));
        assert!(!is_auth_key("tskey-"));
        assert!(!is_auth_key("auth-kBc1CNTRL-abc"));
        assert!(!is_auth_key("tskey-auth abc"));
        assert!(!is_auth_key(&format!("tskey-{}", "a".repeat(251))));
    }
}
