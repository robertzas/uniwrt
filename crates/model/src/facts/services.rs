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
