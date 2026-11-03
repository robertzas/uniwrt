//! Radio events from facts: a radio changing channel (after radar, or not), and its channel
//! staying congested (busy at least `busy_above` % for `busy_after`, clear again at or under
//! `busy_clear` %), as the built-in `channel-busy` alert rule sees it.

use std::collections::BTreeMap;

use uniwrt_core::{NodeId, Timestamp};
use uniwrt_model::{
    events::{EventKind, NewEvent},
    facts::NodeFacts,
};

use crate::{DeriveConfig, conditions::Conditions, derive::key, ms};

/// What a node's radios last showed: each one's channel, and when radar was last reported.
#[derive(Debug, Default)]
pub(crate) struct Radios {
    channels: BTreeMap<String, u16>,
    radar: Option<Timestamp>,
}

impl Radios {
    pub(crate) fn seeded(facts: &NodeFacts) -> Self {
        Self {
            channels: channels(facts).collect(),
            radar: None,
        }
    }

    pub(crate) fn radar(&mut self, at: Timestamp) {
        self.radar = self.radar.max(Some(at));
    }

    pub(crate) fn facts(
        &mut self,
        config: &DeriveConfig,
        conditions: &mut Conditions,
        node: NodeId,
        facts: &NodeFacts,
        now: Timestamp,
    ) -> Vec<NewEvent> {
        let mut events = Vec::new();
        for radio in &facts.radios {
            let busy = key(node, &format!("radio:{}/busy", radio.name));
            let (Some(to), true) = (radio.channel, radio.up && !radio.disabled) else {
                conditions.unknown(&busy);
                continue;
            };
            if let Some(from) = self.channels.insert(radio.name.clone(), to)
                && from != to
            {
                let radar = self
                    .radar
                    .is_some_and(|at| now.saturating_duration_since(at) <= config.radar_window);
                let kind = EventKind::ChannelChanged {
                    radio: radio.name.clone(),
                    band: radio.band,
                    from,
                    to,
                    width: radio.width,
                    radar,
                };
                events.push(NewEvent::new(now, kind).node(node));
            }
            let Some(percent) = radio.channel_busy_percent else {
                continue;
            };
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let busy_percent = percent.clamp(0.0, 100.0).round() as u8;
            let name = radio.name.clone();
            if percent >= config.busy_above {
                events.extend(conditions.bad(&busy, now, config.busy_after, || {
                    let kind = EventKind::ChannelCongested {
                        radio: name,
                        busy_percent,
                    };
                    NewEvent::new(now, kind).node(node)
                }));
            } else if percent <= config.busy_clear {
                events.extend(conditions.good(&busy, now, |lasted| {
                    let kind = EventKind::ChannelClear {
                        radio: name,
                        busy_percent,
                        congested_ms: lasted.map(ms),
                    };
                    NewEvent::new(now, kind).node(node)
                }));
            }
        }
        events
    }
}

fn channels(facts: &NodeFacts) -> impl Iterator<Item = (String, u16)> + '_ {
    facts
        .radios
        .iter()
        .filter(|r| r.up && !r.disabled)
        .filter_map(|r| Some((r.name.clone(), r.channel?)))
}

#[cfg(test)]
#[path = "radios_tests.rs"]
mod tests;
