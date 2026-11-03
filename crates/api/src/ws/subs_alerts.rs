//! The `alerts` topic of a connection: the alerts firing, then each alert stored since (opened,
//! changed, resolved or acknowledged), by id between flushes so that each is sent as it is now.

use std::collections::BTreeMap;

use tracing::warn;
use uniwrt_core::AlertId;
use uniwrt_engine::Event;
use uniwrt_model::alerts::{AlertFilter, AlertRecord};
use uniwrt_store::AlertRepo as _;

use super::{message::ServerMessage, topic::BadTopic};
use crate::Api;

/// The most active alerts a snapshot carries.
const SNAPSHOT: usize = 500;
/// The most alerts a topic keeps between flushes; older ones are dropped.
const MAX_PENDING: usize = 1_000;

pub(super) struct AlertsPending {
    changed: BTreeMap<AlertId, AlertRecord>,
}

impl AlertsPending {
    pub(super) async fn subscribe(
        api: &Api,
        topic: String,
    ) -> Result<(Self, ServerMessage), BadTopic> {
        let filter = AlertFilter {
            active: true,
            ..AlertFilter::default()
        };
        let alerts = api
            .store()
            .alerts(filter, None, SNAPSHOT)
            .await
            .map_err(|err| {
                warn!(%err, "cannot read the alerts for a WS topic");
                BadTopic(
                    "The controller cannot read its alerts; its log has the details".to_owned(),
                )
            })?;
        let pending = Self {
            changed: BTreeMap::new(),
        };
        Ok((pending, ServerMessage::AlertsSnapshot { topic, alerts }))
    }

    pub(super) fn apply(&mut self, event: &Event) {
        let Event::Alerts { alerts } = event else {
            return;
        };
        for alert in alerts.iter() {
            self.changed.insert(alert.id, alert.clone());
        }
        while self.changed.len() > MAX_PENDING {
            self.changed.pop_first();
        }
    }

    pub(super) fn flush(&mut self, topic: String) -> Option<ServerMessage> {
        (!self.changed.is_empty()).then(|| ServerMessage::AlertsDelta {
            topic,
            alerts: std::mem::take(&mut self.changed).into_values().collect(),
        })
    }
}
