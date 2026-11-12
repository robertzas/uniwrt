//! Client steering ([`Deriver::steered`]): what usteer did to a client, or how the client
//! answered a transition request, as a `client_steered` event of the node, with the access
//! point it was steered to placed among the nodes. usteer names another node's access point by
//! the address its usteer talks from, a client's answer by BSSID: each node's facts tell both.
//! Refusals repeat with every request a client sends, so they fold.

use std::net::IpAddr;

use uniwrt_core::{MacAddr, NodeId, Timestamp};
use uniwrt_model::{
    event::{Attachment, SteerAction, SteerEvent, SteerTarget},
    events::{EventKind, NewEvent},
    facts::NodeFacts,
};

use crate::derive::Deriver;

/// Where a node's access points can be found: its addresses and its BSSes.
#[derive(Debug, Default)]
pub(crate) struct Places {
    addresses: Vec<IpAddr>,
    /// By BSSID: the access point's interface.
    bssids: Vec<(MacAddr, String)>,
}

impl Places {
    pub(crate) fn of(facts: &NodeFacts) -> Self {
        let addresses = facts
            .interfaces
            .iter()
            .flat_map(|interface| {
                let v4 = interface.ipv4.iter().map(|net| IpAddr::V4(net.addr()));
                let v6 = interface.ipv6.iter().map(|net| IpAddr::V6(net.addr()));
                v4.chain(v6)
            })
            .collect();
        let bssids = facts
            .radios
            .iter()
            .flat_map(|radio| &radio.ifaces)
            .filter_map(|iface| Some((iface.bssid?, iface.name.clone()?)))
            .collect();
        Self { addresses, bssids }
    }
}

impl Deriver {
    /// Client steering at `node`'s access point, at `at`.
    pub fn steered(&self, node: NodeId, steer: &SteerEvent, at: Timestamp) -> NewEvent {
        let to = steer
            .target
            .as_ref()
            .and_then(|target| self.place(node, target));
        let to_signal_dbm = match &steer.target {
            Some(SteerTarget::Usteer { signal_dbm, .. }) => *signal_dbm,
            _ => None,
        };
        let event = NewEvent::new(
            at,
            EventKind::ClientSteered {
                action: steer.action,
                iface: steer.iface.clone(),
                reason: steer.reason,
                signal_dbm: steer.signal_dbm,
                min_signal_dbm: steer.min_signal_dbm,
                to,
                to_signal_dbm,
                status: steer.status,
            },
        )
        .node(node);
        let Some(mac) = steer.mac else {
            return event;
        };
        let refused = matches!(
            steer.action,
            SteerAction::ProbeDenied | SteerAction::AuthDenied | SteerAction::AssocDenied
        );
        let event = event.client(mac);
        if refused {
            let action = fold_name(steer.action);
            event.fold(format!("steer/{node}/{mac}/{}/{action}", steer.iface))
        } else {
            event
        }
    }

    fn place(&self, node: NodeId, target: &SteerTarget) -> Option<Attachment> {
        let find = |hit: &dyn Fn(&Places) -> Option<String>| {
            self.nodes.iter().find_map(|(&id, state)| {
                Some(Attachment {
                    node: id,
                    iface: hit(&state.places)?,
                })
            })
        };
        match target {
            SteerTarget::Usteer {
                host: None, iface, ..
            } => Some(Attachment {
                node,
                iface: iface.clone(),
            }),
            SteerTarget::Usteer {
                host: Some(host),
                iface,
                ..
            } => find(&|places| places.addresses.contains(host).then(|| iface.clone())),
            SteerTarget::Bssid { bssid } => find(&|places| {
                places
                    .bssids
                    .iter()
                    .find(|(known, _)| known == bssid)
                    .map(|(_, iface)| iface.clone())
            }),
        }
    }
}

/// The action's name in fold keys.
fn fold_name(action: SteerAction) -> &'static str {
    match action {
        SteerAction::ProbeDenied => "probe",
        SteerAction::AuthDenied => "auth",
        SteerAction::AssocDenied => "assoc",
        SteerAction::SignalKick => "signal_kick",
        SteerAction::LoadKick => "load_kick",
        SteerAction::TransitionAccepted => "accepted",
        SteerAction::TransitionRejected => "rejected",
    }
}
