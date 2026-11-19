//! The gateway's Tailscale as `tailscale status --json` reports it (the `services` set; D-140),
//! without the login URL: whoever opens it may join the gateway to their tailnet, so only the
//! login endpoint hands it out, to admins.

use std::net::IpAddr;

use serde::{Deserialize, Serialize};
use uniwrt_core::Timestamp;

/// The most peers the facts list; [`TailscaleFacts::peer_count`] counts them all.
pub const MAX_PEERS: usize = 200;

/// `tailscaled` and the gateway's place in its tailnet.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct TailscaleFacts {
    /// The daemon's state; `None` where it doesn't answer (its service is stopped).
    pub state: Option<TailscaleState>,
    /// Its version, such as `1.104.1-1 (OpenWrt)`.
    pub version: Option<String>,
    /// The tailnet's name and its `MagicDNS` suffix, such as `tail1234.ts.net`.
    pub tailnet: Option<String>,
    pub dns_suffix: Option<String>,
    /// The gateway's name in the tailnet, without the final dot.
    pub dns_name: Option<String>,
    /// Its addresses in the tailnet (100.64.0.0/10 and `fd7a:115c:a1e0::/48`).
    #[cfg_attr(feature = "ts", ts(as = "Vec<String>"))]
    pub addresses: Vec<IpAddr>,
    /// Whether the coordination server sees it.
    pub online: bool,
    /// The routes the tailnet sends to it beyond its own addresses: the subnet routes its admin
    /// approved, and `0.0.0.0/0` and `::/0` once it is approved as an exit node.
    pub approved_routes: Vec<String>,
    /// What `tailscaled` warns about, in its words.
    pub health: Vec<String>,
    /// How many peers the tailnet shows it, of which [`TailscaleFacts::peers`] lists up to
    /// [`MAX_PEERS`], online ones first.
    pub peer_count: u32,
    pub peers: Vec<TailscalePeerFacts>,
}

/// `tailscaled`'s backend state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum TailscaleState {
    NoState,
    /// Logged out: someone must log it in, through the login URL or an auth key.
    NeedsLogin,
    /// Logged in, waiting for the tailnet's admin to approve the machine.
    NeedsMachineAuth,
    /// Logged in and down (`tailscale down`).
    Stopped,
    Starting,
    Running,
    #[serde(other)]
    Unknown,
}

/// Another machine of the tailnet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct TailscalePeerFacts {
    /// Its host name.
    pub name: String,
    /// Its `MagicDNS` name, without the final dot.
    pub dns_name: Option<String>,
    pub os: Option<String>,
    #[cfg_attr(feature = "ts", ts(as = "Vec<String>"))]
    pub addresses: Vec<IpAddr>,
    pub online: bool,
    /// Whether traffic flowed with it lately.
    pub active: bool,
    /// It offers itself as an exit node.
    pub exit_node_option: bool,
    /// The address and port of a direct connection to it; `None` while traffic goes through a
    /// relay or there is none.
    pub direct: Option<String>,
    /// The DERP relay's region it is reached through, such as `fra`.
    pub relay: Option<String>,
    /// When the coordination server last saw it.
    pub last_seen: Option<Timestamp>,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}
