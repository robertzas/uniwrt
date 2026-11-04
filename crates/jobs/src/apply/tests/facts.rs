//! Facts for the apply's unit tests, built field by field.

use uniwrt_core::MacAddr;
use uniwrt_model::{
    facts::{BatmanFacts, BatmanNeighbor, InterfaceFacts, RadioFacts},
    wifi::Band,
};

/// An interface named `name` holding `addr` (`""` for none), with a default route if
/// `gateway`.
pub(crate) fn interface(name: &str, addr: &str, up: bool, gateway: bool) -> InterfaceFacts {
    InterfaceFacts {
        name: name.to_owned(),
        proto: "dhcp".to_owned(),
        up,
        device: Some("br-lan".to_owned()),
        uptime_s: None,
        ipv4: if addr.is_empty() {
            Vec::new()
        } else {
            vec![addr.parse().unwrap()]
        },
        ipv6: Vec::new(),
        delegated_prefixes: Vec::new(),
        ipv4_gateway: gateway.then(|| "192.0.2.1".parse().unwrap()),
        ipv6_gateway: None,
        dns_servers: Vec::new(),
        errors: Vec::new(),
    }
}

pub(crate) fn radio(name: &str, up: bool, disabled: bool) -> RadioFacts {
    RadioFacts {
        name: name.to_owned(),
        phy: None,
        path: None,
        band: Band::G5,
        up,
        disabled,
        channel: None,
        frequency_mhz: None,
        center_frequency_mhz: None,
        width: None,
        standard: None,
        tx_power_dbm: None,
        noise_dbm: None,
        channel_busy_percent: None,
        country: None,
        ifaces: Vec::new(),
    }
}

/// A mesh hearing `neighbors` nodes directly.
pub(crate) fn mesh(neighbors: u8) -> BatmanFacts {
    BatmanFacts {
        mesh_if: "bat0".to_owned(),
        mac: None,
        algorithm: None,
        gw_mode: None,
        hard_ifs: Vec::new(),
        originators: Vec::new(),
        neighbors: (0..neighbors)
            .map(|i| BatmanNeighbor {
                mac: MacAddr::new([2, 0, 0, 0, 0, i]),
                hard_if: "uw_mesh".to_owned(),
                last_seen_ms: 100,
                throughput_kbps: None,
            })
            .collect(),
        gateways: Vec::new(),
        remote_clients: Vec::new(),
        local_clients: Vec::new(),
    }
}
