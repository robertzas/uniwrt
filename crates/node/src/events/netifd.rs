//! netifd's notifications: `interface.update` (up, with what changed) and `interface.down` from
//! `network.interface`, each with the interface's status, and the `network.device` changes with
//! the device's state.

use serde::Deserialize;
use uniwrt_model::event::{DeviceChange, DeviceEvent, InterfaceEvent, NodeEvent};

use super::decode;

#[derive(Deserialize)]
struct Interface {
    #[serde(rename = "interface")]
    name: String,
    up: bool,
    l3_device: Option<String>,
    device: Option<String>,
    #[serde(default)]
    updated: Vec<String>,
}

#[derive(Deserialize)]
struct Device {
    name: String,
    #[serde(default)]
    present: bool,
    #[serde(default)]
    active: bool,
    #[serde(default)]
    link_active: bool,
}

pub(super) fn interface(kind: &str, raw: &str) -> Result<Option<NodeEvent>, String> {
    if kind != "interface.update" && kind != "interface.down" {
        return Ok(None);
    }
    let raw: Interface = decode(raw)?;
    Ok(Some(NodeEvent::Interface(InterfaceEvent {
        name: raw.name,
        up: raw.up,
        // A down interface has no layer 3 device, only the one it is configured on.
        device: raw.l3_device.or(raw.device),
        updated: raw.updated,
    })))
}

pub(super) fn device(kind: &str, raw: &str) -> Result<Option<NodeEvent>, String> {
    let change = match kind {
        "add" => DeviceChange::Added,
        "remove" => DeviceChange::Removed,
        "up" => DeviceChange::Up,
        "down" => DeviceChange::Down,
        "link_up" => DeviceChange::LinkUp,
        "link_down" => DeviceChange::LinkDown,
        "topo_change" => DeviceChange::TopologyChanged,
        _ => return Ok(None),
    };
    let raw: Device = decode(raw)?;
    Ok(Some(NodeEvent::Device(DeviceEvent {
        name: raw.name,
        change,
        present: raw.present,
        active: raw.active,
        carrier: raw.link_active,
    })))
}
