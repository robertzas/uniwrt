//! The `events` topic of a connection: the newest events recorded, then each one recorded or
//! folded since that passes its filter (one node's, at a least level), by id between flushes so
//! that a fold sends the record as it is now.

use std::collections::BTreeMap;

use tracing::warn;
use uniwrt_core::{EventId, NodeId};
use uniwrt_engine::Event;
use uniwrt_model::events::{EventFilter, EventLevel, EventRecord};
use uniwrt_store::EventRepo as _;

use super::{message::ServerMessage, topic::BadTopic};
use crate::Api;

/// The events a topic's snapshot carries, the newest.
const SNAPSHOT: usize = 100;
/// The most events a topic keeps between flushes; older ones are dropped (the client can read
/// them back).
const MAX_PENDING: usize = 1_000;

pub(super) struct EventsPending {
    filter: EventFilter,
    changed: BTreeMap<EventId, EventRecord>,
}

impl EventsPending {
    pub(super) async fn subscribe(
        api: &Api,
        topic: String,
        node: Option<NodeId>,
        min: EventLevel,
    ) -> Result<(Self, ServerMessage), BadTopic> {
        let filter = EventFilter {
            nodes: node.into_iter().collect(),
            min: (min != EventLevel::Info).then_some(min),
            ..EventFilter::default()
        };
        let mut events = api
            .store()
            .events(filter.clone(), None, SNAPSHOT)
            .await
            .map_err(|err| {
                warn!(%err, "cannot read the events for a WS topic");
                BadTopic(
                    "The controller cannot read its events; its log has the details".to_owned(),
                )
            })?;
        events.reverse();
        let pending = Self {
            filter,
            changed: BTreeMap::new(),
        };
        Ok((pending, ServerMessage::EventsSnapshot { topic, events }))
    }

    pub(super) fn apply(&mut self, event: &Event) {
        let Event::Recorded { events } = event else {
            return;
        };
        for record in events.iter().filter(|record| self.filter.admits(record)) {
            self.changed.insert(record.id, record.clone());
        }
        while self.changed.len() > MAX_PENDING {
            self.changed.pop_first();
        }
    }

    pub(super) fn flush(&mut self, topic: String) -> Option<ServerMessage> {
        (!self.changed.is_empty()).then(|| ServerMessage::EventsDelta {
            topic,
            events: std::mem::take(&mut self.changed).into_values().collect(),
        })
    }
}
