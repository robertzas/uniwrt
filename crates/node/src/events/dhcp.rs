//! The DHCP servers' notifications, alike for dnsmasq and odhcpd: `dhcp.ack` with the client's
//! `mac`, `ip`, `name` and the `interface` it was leased on, and `dhcp.release` without the name.

use std::net::IpAddr;

use serde::Deserialize;
use uniwrt_model::event::{LeaseChange, LeaseEvent, NodeEvent};

use super::decode;

#[derive(Deserialize)]
struct Raw {
    ip: IpAddr,
    mac: Option<String>,
    name: Option<String>,
    interface: Option<String>,
}

pub(super) fn parse(kind: &str, raw: &str) -> Result<Option<NodeEvent>, String> {
    let change = match kind {
        "dhcp.ack" => LeaseChange::Acked,
        "dhcp.release" => LeaseChange::Released,
        _ => return Ok(None),
    };
    let raw: Raw = decode(raw)?;
    Ok(Some(NodeEvent::Lease(LeaseEvent {
        change,
        ip: raw.ip,
        // A client identified otherwise (by a DUID, over InfiniBand) has no usable MAC.
        mac: raw.mac.and_then(|mac| mac.parse().ok()),
        hostname: raw.name.filter(|name| !name.is_empty()),
        device: raw.interface,
    })))
}
