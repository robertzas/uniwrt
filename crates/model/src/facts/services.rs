//! The gateway's network services as they run (the `services` set; D-132).

use std::net::IpAddr;

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
