//! The gateway's network services as they run (the `services` set; D-132).

use std::net::{IpAddr, SocketAddr};

use serde::{Deserialize, Serialize};
use uniwrt_core::Timestamp;

/// What the gateway's services report.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ServiceFacts {
    /// Each ddns-scripts section, by name.
    pub ddns: Vec<DdnsFacts>,
    /// How many https-dns-proxy instances run; `None` where procd has no such service.
    pub doh_instances: Option<u32>,
    /// The DNS blocklist the controller pushed (D-133), as the gateway's files say.
    #[serde(default)]
    pub blocklist: BlocklistFacts,
    /// The WireGuard tunnel `vpn` (D-140); `None` where the gateway has no such device.
    #[serde(default)]
    pub wireguard: Option<WireguardFacts>,
}

/// The tunnel as `wg show vpn dump` reports it, without its private and preshared keys.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct WireguardFacts {
    /// The node's clock when it was read, against which the handshakes are timed.
    pub node_time: Option<Timestamp>,
    /// The public half of the key the tunnel runs with.
    pub public_key: Option<String>,
    /// The UDP port it listens on.
    pub port: Option<u16>,
    pub peers: Vec<WireguardPeerFacts>,
}

/// One peer of the tunnel, by its public key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct WireguardPeerFacts {
    pub public_key: String,
    /// Where its packets last came from (or where the gateway dials it); `None` before any.
    #[cfg_attr(feature = "ts", ts(as = "Option<String>"))]
    pub endpoint: Option<SocketAddr>,
    /// Its latest handshake, on the node's clock; `None` if it never made one.
    pub handshake_at: Option<Timestamp>,
    /// Bytes received from it and sent to it since the tunnel came up.
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

/// The blocklist's files on the gateway (`/tmp/dnsmasq.uniwrt.d`, in RAM: gone after a reboot).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct BlocklistFacts {
    /// The SHA-256 the list's header line names (`model::blocklists::header`); `None` without
    /// the file or its header.
    pub sha256: Option<String>,
    /// The domains its header counts.
    pub domains: Option<u32>,
    /// Whether the conf file that loads it into dnsmasq is in place.
    pub conf: bool,
}

/// One DDNS name's state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct DdnsFacts {
    /// Its section, the site's DDNS id.
    pub id: String,
    /// The address last registered with the provider.
    #[cfg_attr(feature = "ts", ts(as = "Option<String>"))]
    pub address: Option<IpAddr>,
    /// When it was last sent, on the node's clock.
    pub updated_at: Option<Timestamp>,
    /// Whether its updater runs.
    pub running: bool,
}
