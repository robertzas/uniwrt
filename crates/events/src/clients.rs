//! Clients' comings and goings, from their views: a client seen for the first time is new, one
//! that comes online joined, one that goes offline left. Nothing is derived during a node's
//! first minute online, whose station lists show clients that were there all along (nor, for
//! clients no node places, during the deriver's first minute), nor about clients leaving a node
//! that went offline.

use std::sync::Arc;

use uniwrt_clients::Client;
use uniwrt_core::{MacAddr, NodeId, Timestamp};
use uniwrt_model::events::{EventKind, NewEvent};

use crate::{Deriver, ms};

/// What the deriver last saw of a client.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Seen {
    online: bool,
    node: Option<NodeId>,
    /// Since when it is at its connection.
    since: Option<Timestamp>,
}

impl Seen {
    pub(crate) fn of(client: &Client) -> Self {
        let connection = client.connection.as_ref();
        Self {
            online: client.online,
            node: connection.and_then(|connection| connection.node),
            since: connection.map(|connection| connection.since),
        }
    }
}

impl Deriver {
    /// Clients' views that changed, as they are at `now`.
    pub fn clients(&mut self, clients: &[Arc<Client>], now: Timestamp) -> Vec<NewEvent> {
        let mut events = Vec::new();
        for client in clients {
            let seen = Seen::of(client);
            let before = self.clients.insert(client.mac, seen);
            let event = match before {
                None if client.online && self.settled(seen.node, now) => Some(NewEvent::new(
                    now,
                    EventKind::NewClient {
                        vendor: client.vendor.clone(),
                        hostname: client.hostname.clone(),
                    },
                )),
                Some(before) if !before.online && client.online && self.settled(seen.node, now) => {
                    let connection = client.connection.as_ref();
                    connection.map(|connection| {
                        NewEvent::new(
                            now,
                            EventKind::ClientJoined {
                                medium: connection.medium,
                                place: connection.port.clone(),
                                ssid: connection.ssid.clone(),
                                band: connection.band,
                            },
                        )
                    })
                }
                Some(before)
                    if before.online
                        && !client.online
                        && before.node.is_none_or(|node| self.is_online(node)) =>
                {
                    let connected_ms = before
                        .since
                        .filter(|since| *since <= now)
                        .map(|since| ms(now.saturating_duration_since(since)));
                    let event = NewEvent::new(now, EventKind::ClientLeft { connected_ms });
                    Some(match before.node {
                        Some(node) => event.node(node),
                        None => event,
                    })
                }
                _ => None,
            };
            if let Some(mut event) = event {
                if event.node.is_none() {
                    event.node = seen.node;
                }
                events.push(event.client(client.mac));
            }
        }
        events
    }

    /// Clients forgotten (long gone).
    pub fn clients_removed(&mut self, macs: &[MacAddr]) {
        for mac in macs {
            self.clients.remove(mac);
        }
    }

    pub(crate) fn is_online(&self, node: NodeId) -> bool {
        self.nodes
            .get(&node)
            .is_some_and(|state| state.online_since.is_some())
    }

    /// Whether the node a client is at has been online for a while; for a client that no node
    /// places (behind a mesh peer that is not adopted, or seen only in leases), whether the
    /// deriver has run for a while.
    fn settled(&self, node: Option<NodeId>, now: Timestamp) -> bool {
        let settle = self.config.settle;
        match node {
            None => now.saturating_duration_since(self.started) >= settle,
            Some(node) => self
                .nodes
                .get(&node)
                .and_then(|state| state.online_since)
                .is_some_and(|since| now.saturating_duration_since(since) >= settle),
        }
    }
}
