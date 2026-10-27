//! Observed facts: everything one collection round of the node agent reports about a node.

mod capabilities;
mod interface;
mod radio;
mod station;
mod system;

use serde::{Deserialize, Serialize};
use uniwrt_core::Timestamp;

pub use self::{
    capabilities::{Capabilities, PackageManager, Software, SwitchKind, WifiFeature},
    interface::{Counters, DeviceFacts, DeviceKind, InterfaceFacts},
    radio::{Channel, RadioCapabilities, RadioFacts, WifiIfaceFacts, WifiIfaceMode},
    station::{Rate, StationCapability, StationFacts},
    system::{BoardFacts, Memory, Release, Storage, SystemFacts},
};

/// The observed state of one node.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct NodeFacts {
    /// When the facts were collected, on the controller's clock (the node's own clock is
    /// [`SystemFacts::node_time`]).
    pub collected_at: Timestamp,
    pub board: BoardFacts,
    pub system: SystemFacts,
    pub capabilities: Capabilities,
    /// netifd's logical interfaces (`lan`, `wan`, `wan6`).
    pub interfaces: Vec<InterfaceFacts>,
    /// Kernel network devices (`lan1`, `br-lan`, `phy1-ap0`, `bat0`).
    pub devices: Vec<DeviceFacts>,
    pub radios: Vec<RadioFacts>,
    /// Associated Wi-Fi stations and mesh peers of every radio, each naming its interface.
    pub stations: Vec<StationFacts>,
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::NodeFacts;

    const FIXTURES: [(&str, &str); 1] =
        [("gateway", include_str!("../../fixtures/facts/gateway.json"))];

    /// Every fixture field must survive parsing: a misspelled or stale field in a fixture, or a
    /// field the model silently drops, fails here.
    #[test]
    fn fixtures_round_trip_without_losing_fields() {
        for (name, json) in FIXTURES {
            let facts: NodeFacts =
                serde_json::from_str(json).unwrap_or_else(|err| panic!("{name}: {err}"));
            // Compared as re-parsed text: `to_value` would widen f32 fields to inexact f64s.
            let reparsed: Value =
                serde_json::from_str(&serde_json::to_string(&facts).unwrap()).unwrap();
            let original: Value = serde_json::from_str(json).unwrap();
            assert_eq!(reparsed, original, "{name}");
        }
    }

    #[test]
    fn default_facts_round_trip() {
        let facts = NodeFacts::default();
        let json = serde_json::to_string(&facts).unwrap();
        assert_eq!(serde_json::from_str::<NodeFacts>(&json).unwrap(), facts);
    }
}
