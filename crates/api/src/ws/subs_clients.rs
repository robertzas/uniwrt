//! The clients' topics of a connection: `clients` keeps each changed client's latest view by
//! MAC address between flushes, and `client:<mac>` one client's view, its new samples (up to a
//! cap) and its changed sessions, with fast telemetry for the node it is at.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::Arc,
};

use tracing::warn;
use uniwrt_clients::{Client, ClientSample};
use uniwrt_core::{MacAddr, NodeId, Timestamp};
use uniwrt_engine::{Event, Interest};
use uniwrt_model::client::ClientSession;

use super::{message::ServerMessage, subs::push_capped, topic::BadTopic};
use crate::{Api, clients};

/// The `clients` topic's changes since the last flush.
#[derive(Default)]
pub(super) struct ClientsPending {
    changed: BTreeMap<MacAddr, Arc<Client>>,
    removed: BTreeSet<MacAddr>,
}

impl ClientsPending {
    pub(super) fn subscribe(api: &Api, topic: String) -> (Self, ServerMessage) {
        let snapshot = ServerMessage::ClientsSnapshot {
            topic,
            clients: api.fleet().clients(),
        };
        (Self::default(), snapshot)
    }

    pub(super) fn apply(&mut self, event: &Event) {
        let Event::Clients { changes } = event else {
            return;
        };
        for client in &changes.clients {
            self.removed.remove(&client.mac);
            self.changed.insert(client.mac, Arc::clone(client));
        }
        for mac in &changes.removed {
            self.changed.remove(mac);
            self.removed.insert(*mac);
        }
    }

    pub(super) fn flush(&mut self, topic: String) -> Option<ServerMessage> {
        (!self.changed.is_empty() || !self.removed.is_empty()).then(|| {
            ServerMessage::ClientsDelta {
                topic,
                clients: std::mem::take(&mut self.changed).into_values().collect(),
                removed: std::mem::take(&mut self.removed).into_iter().collect(),
            }
        })
    }
}

/// The `client:<mac>` topic's changes since the last flush.
pub(super) struct ClientPending {
    mac: MacAddr,
    client: Option<Arc<Client>>,
    samples: VecDeque<ClientSample>,
    /// By start.
    sessions: BTreeMap<Timestamp, ClientSession>,
    removed: bool,
    /// Fast telemetry for the node the client is at, which follows it as it roams.
    focus: Option<(NodeId, Interest)>,
}

impl ClientPending {
    pub(super) async fn subscribe(
        api: &Api,
        topic: String,
        mac: MacAddr,
    ) -> Result<(Self, ServerMessage), BadTopic> {
        let detail = clients::detail(api, mac)
            .await
            .map_err(|err| {
                warn!(?err, "cannot read a client's sessions for a WS topic");
                BadTopic(
                    "The controller cannot read the client's sessions; its log has the details"
                        .to_owned(),
                )
            })?
            .ok_or_else(|| BadTopic(format!("There is no client {mac}")))?;
        let pending = Self {
            mac,
            client: None,
            samples: VecDeque::new(),
            sessions: BTreeMap::new(),
            removed: false,
            focus: focus(api, &detail.client),
        };
        let snapshot = ServerMessage::ClientSnapshot {
            topic,
            client: detail,
        };
        Ok((pending, snapshot))
    }

    pub(super) fn apply(&mut self, api: &Api, event: &Event) {
        let Event::Clients { changes } = event else {
            return;
        };
        let mac = self.mac;
        if let Some(view) = changes.clients.iter().find(|view| view.mac == mac) {
            if self.focus.as_ref().map(|(node, _)| *node) != node_of(view) {
                self.focus = focus(api, view);
            }
            self.client = Some(Arc::clone(view));
        }
        for (_, sample) in changes.samples.iter().filter(|(of, _)| *of == mac) {
            push_capped(&mut self.samples, *sample);
        }
        for session in changes.sessions.iter().filter(|session| session.mac == mac) {
            self.sessions.insert(session.start, session.clone());
        }
        if changes.removed.contains(&mac) {
            self.removed = true;
        }
    }

    pub(super) fn flush(&mut self, topic: String) -> Option<ServerMessage> {
        if self.client.is_none()
            && self.samples.is_empty()
            && self.sessions.is_empty()
            && !self.removed
        {
            return None;
        }
        Some(ServerMessage::ClientDelta {
            topic,
            client: self.client.take(),
            samples: std::mem::take(&mut self.samples).into(),
            sessions: std::mem::take(&mut self.sessions).into_values().collect(),
            removed: std::mem::take(&mut self.removed),
        })
    }
}

/// The node an online client is at.
fn node_of(client: &Client) -> Option<NodeId> {
    client.connection.as_ref().filter(|_| client.online)?.node
}

/// Fast telemetry for the node an online client is at.
fn focus(api: &Api, client: &Client) -> Option<(NodeId, Interest)> {
    let node = node_of(client)?;
    Some((node, api.fleet().focus(node)?))
}
