//! What travels over `/api/v1/ws`: the client's [`ClientMessage`] and the server's
//! [`ServerMessage`]s, tagged by `type` and naming the topic as the client spelled it.

use std::{collections::BTreeSet, sync::Arc};

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uniwrt_core::{NodeId, Timestamp};
use uniwrt_model::{
    event::{NodeEvent, Roam},
    facts::{
        BatmanFacts, BoardFacts, Capabilities, DeviceFacts, FdbEntry, InterfaceFacts, LeaseFacts,
        NeighborFacts, NodeFacts, PhyFacts, RadioFacts, StationFacts, SystemFacts,
    },
    job::Job,
    log::LogEntry,
    status::NodeStatus,
};
use uniwrt_node::proto::ops::Set;
use uniwrt_topo::{Topology, TopologyDelta};

use crate::nodes::{NodeDetail, NodeSummary};

/// Replaces the connection's topics with these: new ones answer with a snapshot, and topics
/// left out stop.
#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct ClientMessage {
    pub sub: Vec<String>,
}

#[derive(Debug, Serialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum ServerMessage {
    /// Every node, when `fleet` is subscribed and after a resync.
    FleetSnapshot {
        topic: String,
        nodes: Vec<NodeSummary>,
    },
    /// Nodes whose summary changed, and nodes no longer adopted.
    FleetDelta {
        topic: String,
        nodes: Vec<NodeSummary>,
        removed: Vec<NodeId>,
    },
    NodeSnapshot {
        topic: String,
        node: NodeDetail,
    },
    /// What changed since the last message: the status, the parts of the facts that changed,
    /// and what happened, oldest first.
    NodeDelta {
        topic: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        status: Option<NodeStatus>,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        facts: Option<Box<FactsPatch>>,
        events: Vec<TimedEvent>,
        /// Roams from or to the node.
        roams: Vec<TimedRoam>,
        /// The controller stopped watching the node.
        removed: bool,
    },
    /// The entries kept before the subscription (none until logs are stored, M2.5).
    LogSnapshot {
        topic: String,
        entries: Vec<LogEntry>,
    },
    /// New entries, oldest first; `dropped` counts those lost before them.
    LogDelta {
        topic: String,
        entries: Vec<LogEntry>,
        dropped: u64,
    },
    /// The latest jobs, newest first.
    JobsSnapshot {
        topic: String,
        jobs: Vec<Job>,
    },
    /// Jobs that started or changed, each as it is now.
    JobsDelta {
        topic: String,
        jobs: Vec<Job>,
    },
    /// The fleet's topology, when `topology` is subscribed and after a resync.
    TopologySnapshot {
        topic: String,
        topology: Arc<Topology>,
    },
    /// What changed in the topology since the last message.
    TopologyDelta {
        topic: String,
        delta: TopologyDelta,
    },
    /// A topic was refused (and is not subscribed), or the message was not understood (no
    /// topic).
    Error {
        topic: Option<String>,
        message: String,
    },
}

/// The parts of a node's facts that changed: replace each one present.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct FactsPatch {
    pub collected_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub board: Option<BoardFacts>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub system: Option<SystemFacts>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub capabilities: Option<Capabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub interfaces: Option<Vec<InterfaceFacts>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub devices: Option<Vec<DeviceFacts>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub phys: Option<Vec<PhyFacts>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub radios: Option<Vec<RadioFacts>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub stations: Option<Vec<StationFacts>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub leases: Option<Vec<LeaseFacts>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub neighbors: Option<Vec<NeighborFacts>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub fdb: Option<Vec<FdbEntry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub batman: Option<Vec<BatmanFacts>>,
}

impl FactsPatch {
    /// The parts of `facts` that the collect `sets` fill.
    pub(crate) fn new(facts: &Arc<NodeFacts>, sets: &BTreeSet<Set>) -> Self {
        let has = |set| sets.contains(&set);
        Self {
            collected_at: facts.collected_at,
            board: has(Set::Board).then(|| facts.board.clone()),
            system: has(Set::System).then(|| facts.system.clone()),
            capabilities: has(Set::Capabilities).then(|| facts.capabilities.clone()),
            interfaces: has(Set::Interfaces).then(|| facts.interfaces.clone()),
            devices: has(Set::Devices).then(|| facts.devices.clone()),
            phys: has(Set::Phys).then(|| facts.phys.clone()),
            radios: has(Set::Radios).then(|| facts.radios.clone()),
            stations: has(Set::Stations).then(|| facts.stations.clone()),
            leases: has(Set::Leases).then(|| facts.leases.clone()),
            neighbors: has(Set::Neighbors).then(|| facts.neighbors.clone()),
            fdb: has(Set::Neighbors).then(|| facts.fdb.clone()),
            batman: has(Set::Batman).then(|| facts.batman.clone()),
        }
    }
}

/// Something a node reported, as the engine stamped it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct TimedEvent {
    /// When it arrived, on the controller's clock.
    pub at: Timestamp,
    /// When it happened, on the node's clock (which may be skewed).
    pub reported_at: Option<Timestamp>,
    pub event: NodeEvent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct TimedRoam {
    pub at: Timestamp,
    pub roam: Roam,
}
