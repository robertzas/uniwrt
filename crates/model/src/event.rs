//! What nodes report as it happens: the node agent's push events (ARCHITECTURE.md, Node
//! protocol), which the `node` crate parses from hostapd's, netifd's and the DHCP servers' ubus
//! notifications. They carry no node or time: the engine stamps them as they arrive. A
//! [`Roam`] is what the engine infers from them across nodes.

use std::net::IpAddr;

use serde::{Deserialize, Serialize};
use uniwrt_core::{MacAddr, NodeId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NodeEvent {
    /// A Wi-Fi client joining or leaving one of the node's access points (hostapd).
    Station(StationEvent),
    /// A kernel device appearing, going away, going up or down, or its link changing (netifd).
    Device(DeviceEvent),
    /// A logical interface going up, down, or changing while up (netifd).
    Interface(InterfaceEvent),
    /// A DHCP server acking or releasing a lease (dnsmasq, odhcpd).
    Lease(LeaseEvent),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StationEvent {
    pub mac: MacAddr,
    /// The access point's interface, such as `phy1-ap0`.
    pub iface: String,
    pub change: StationChange,
    /// The signal of the request (auth and assoc requests).
    pub signal_dbm: Option<i16>,
    /// The frequency the request came on (auth and assoc requests).
    pub freq_mhz: Option<u32>,
    /// How the station authenticated (authorized only), as hostapd names it: `open`, `sae`,
    /// `ft` (an 802.11r fast transition), `fils`…
    pub auth_alg: Option<String>,
}

/// A station's steps through joining, in order, and its leaving.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StationChange {
    /// It asked to authenticate.
    AuthRequest,
    /// It asked to associate or reassociate.
    AssocRequest,
    /// Its port opened once the key handshake completed: it is connected.
    Authorized,
    /// It is gone: it left or was disconnected.
    Disassociated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceEvent {
    pub name: String,
    pub change: DeviceChange,
    /// The kernel has the device.
    pub present: bool,
    /// netifd has brought it up.
    pub active: bool,
    /// It has a link (carrier).
    pub carrier: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceChange {
    Added,
    Removed,
    Up,
    Down,
    LinkUp,
    LinkDown,
    /// A bridge's ports changed.
    TopologyChanged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterfaceEvent {
    /// The interface's name in UCI, such as `lan` or `wan`.
    pub name: String,
    pub up: bool,
    /// The device carrying its layer 3, such as `br-lan`.
    pub device: Option<String>,
    /// What netifd says changed, such as `addresses`, `routes` or `prefixes`; empty when it
    /// went down.
    pub updated: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaseEvent {
    pub change: LeaseChange,
    pub ip: IpAddr,
    pub mac: Option<MacAddr>,
    /// The name the client asked for (acks only).
    pub hostname: Option<String>,
    /// The device it was leased on, such as `br-lan`.
    pub device: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeaseChange {
    /// Handed out or renewed.
    Acked,
    /// Given back by the client.
    Released,
}

/// A Wi-Fi client moving from one access point to another, on the same node or another one:
/// it was authorized at `to` while it was still connected at `from`, or shortly after leaving
/// it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Roam {
    pub mac: MacAddr,
    pub from: Attachment,
    pub to: Attachment,
    /// How it authenticated at `to`; `ft` is an 802.11r fast transition.
    pub auth_alg: Option<String>,
}

/// An access point of a node: the interface a client is connected to.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Attachment {
    pub node: NodeId,
    /// Such as `phy1-ap0`.
    pub iface: String,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{NodeEvent, StationChange, StationEvent};

    #[test]
    fn events_are_tagged_by_kind() {
        let event = NodeEvent::Station(StationEvent {
            mac: "02:00:00:00:04:00".parse().unwrap(),
            iface: "phy1-ap0".to_owned(),
            change: StationChange::Authorized,
            signal_dbm: None,
            freq_mhz: None,
            auth_alg: Some("sae".to_owned()),
        });
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(
            value,
            json!({
                "kind": "station", "mac": "02:00:00:00:04:00", "iface": "phy1-ap0",
                "change": "authorized", "signal_dbm": null, "freq_mhz": null, "auth_alg": "sae"
            })
        );
        assert_eq!(serde_json::from_value::<NodeEvent>(value).unwrap(), event);
    }
}
