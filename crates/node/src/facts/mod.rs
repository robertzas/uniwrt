//! Parsing the agent's collect sets into [`NodeFacts`]: one parser per [`Set`], over the raw
//! sources that `node-agent/collect/<set>.uc` gathers. Parsers are pure. They are lenient where
//! OpenWrt versions differ (a missing field becomes `None` or empty, an address that does not
//! parse is skipped) and strict where the shape is wrong (a set fails with a [`FactsError`]).

mod batman;
mod board;
mod capabilities;
mod devices;
mod interfaces;
mod leases;
mod neighbors;
mod phys;
mod radios;
mod stations;
mod system;
mod wifi;

use serde::de::DeserializeOwned;
use serde_json::value::RawValue;
use uniwrt_model::facts::{
    BatmanFacts, BoardFacts, Capabilities, DeviceFacts, DeviceKind, FdbEntry, InterfaceFacts,
    LeaseFacts, NeighborFacts, NodeFacts, PhyFacts, RadioFacts, StationFacts, SystemFacts,
};

use crate::proto::ops::{Collected, Set};

/// The facts of one set, ready to be applied to a [`NodeFacts`].
#[derive(Debug, Clone, PartialEq)]
pub enum SetFacts {
    Batman(Vec<BatmanFacts>),
    Board(BoardFacts),
    Capabilities(Capabilities),
    Devices(Vec<DeviceFacts>),
    Interfaces(Vec<InterfaceFacts>),
    Leases(Vec<LeaseFacts>),
    Neighbors {
        neighbors: Vec<NeighborFacts>,
        fdb: Vec<FdbEntry>,
    },
    Phys(Vec<PhyFacts>),
    Radios(Vec<RadioFacts>),
    Stations(Vec<StationFacts>),
    System(SystemFacts),
}

impl SetFacts {
    pub fn parse(set: Set, raw: &RawValue) -> Result<Self, FactsError> {
        let raw = raw.get();
        Ok(match set {
            Set::Batman => Self::Batman(batman::parse(decode(set, raw)?)),
            Set::Board => Self::Board(board::parse(decode(set, raw)?)),
            Set::Capabilities => Self::Capabilities(capabilities::parse(decode(set, raw)?)),
            Set::Devices => Self::Devices(devices::parse(decode(set, raw)?)),
            Set::Interfaces => Self::Interfaces(interfaces::parse(decode(set, raw)?)),
            Set::Leases => Self::Leases(leases::parse(decode(set, raw)?)),
            Set::Neighbors => {
                let (neighbors, fdb) = neighbors::parse(decode(set, raw)?);
                Self::Neighbors { neighbors, fdb }
            }
            Set::Phys => Self::Phys(phys::parse(decode(set, raw)?)),
            Set::Radios => Self::Radios(radios::parse(decode(set, raw)?)),
            Set::Stations => Self::Stations(stations::parse(decode(set, raw)?)),
            Set::System => Self::System(system::parse(decode(set, raw)?)),
        })
    }

    /// Replaces the part of `facts` that this set covers.
    pub fn apply(self, facts: &mut NodeFacts) {
        match self {
            Self::Batman(meshes) => {
                facts.batman = meshes;
                mark_meshes(facts);
            }
            Self::Board(board) => facts.board = board,
            Self::Capabilities(capabilities) => facts.capabilities = capabilities,
            Self::Devices(devices) => {
                facts.devices = devices;
                mark_meshes(facts);
            }
            Self::Interfaces(interfaces) => facts.interfaces = interfaces,
            Self::Leases(leases) => facts.leases = leases,
            Self::Neighbors { neighbors, fdb } => {
                facts.neighbors = neighbors;
                facts.fdb = fdb;
            }
            Self::Phys(phys) => facts.phys = phys,
            Self::Radios(radios) => facts.radios = radios,
            Self::Stations(stations) => facts.stations = stations,
            Self::System(system) => facts.system = system,
        }
    }
}

/// batman-adv mesh devices look like any other device in sysfs; the `batman` set knows them.
fn mark_meshes(facts: &mut NodeFacts) {
    for device in &mut facts.devices {
        if facts.batman.iter().any(|mesh| mesh.mesh_if == device.name) {
            device.kind = DeviceKind::Batman;
        }
    }
}

/// A set that could not be collected on the node or parsed here.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("collect set {set}: {reason}")]
pub struct FactsError {
    pub set: Set,
    pub reason: String,
}

/// Applies every set of a collect answer to `facts`, leaving the parts of failed sets as they
/// were; returns why those failed.
pub fn apply(collected: &Collected, facts: &mut NodeFacts) -> Vec<FactsError> {
    let mut errors: Vec<_> = collected
        .errors
        .iter()
        .map(|(&set, err)| FactsError {
            set,
            reason: format!("failed on the node: {}: {}", err.code, err.msg),
        })
        .collect();
    for (&set, raw) in &collected.sets {
        match SetFacts::parse(set, raw) {
            Ok(parsed) => parsed.apply(facts),
            Err(err) => errors.push(err),
        }
    }
    errors
}

fn decode<T: DeserializeOwned>(set: Set, raw: &str) -> Result<T, FactsError> {
    serde_json::from_str(raw).map_err(|err| FactsError {
        set,
        reason: err.to_string(),
    })
}

/// `value` rounded to `decimals` places: the precision the sources carry, and short JSON.
#[expect(clippy::cast_possible_truncation, reason = "facts carry f32")]
fn round(value: f64, decimals: i32) -> f32 {
    let scale = 10_f64.powi(decimals);
    ((value * scale).round() / scale) as f32
}

#[cfg(test)]
mod tests;
