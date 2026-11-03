//! What to send, and when (features/observability.md, Notifications): pure, fed the alerts as
//! the alert engine stores them and ticked for reminders.
//!
//! - An alert that opens (or reopens) is sent once to each enabled notifier whose filter admits
//!   it, unless it is silenced or acknowledged; one silenced when it opened is sent once it no
//!   longer is.
//! - While it fires, neither acknowledged nor silenced, each notifier that heard of it is
//!   reminded at its interval.
//! - When it resolves, the notifiers that heard of it are told, if they want resolutions. An
//!   alert resolved because its rule changed tells nobody: the alert that replaces it within
//!   [`CARRY_OVER`] takes over what was sent (an edited rule's alerts start afresh).
//! - A notifier added or changed hears of the alerts that open from then on, not of those
//!   already firing. After a restart, the alerts still firing count as heard of
//!   ([`Policy::resume`]).

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    time::Duration,
};

use uniwrt_core::{AlertId, NotifierId, RuleId, Timestamp};
use uniwrt_model::{
    alerts::{AlertRecord, AlertResolution},
    notify::NotifierFilter,
};

use crate::message::Kind;

/// How long after an alert resolved because its rule changed its replacement takes over.
pub const CARRY_OVER: Duration = Duration::from_mins(2);

/// One alert to send to one notifier.
#[derive(Debug, Clone, PartialEq)]
pub struct Send {
    pub notifier: NotifierId,
    pub kind: Kind,
    pub alert: AlertRecord,
}

#[derive(Debug, Default)]
pub struct Policy {
    /// The enabled notifiers.
    notifiers: BTreeMap<NotifierId, NotifierFilter>,
    /// The alerts firing.
    alerts: BTreeMap<AlertId, Tracked>,
    /// What was sent of alerts resolved because their rule changed, by rule and subject, with
    /// when they resolved.
    replaced: HashMap<(RuleId, String), (Timestamp, BTreeMap<NotifierId, Timestamp>)>,
}

#[derive(Debug)]
struct Tracked {
    alert: AlertRecord,
    /// How many times it had fired when last seen: more means it reopened.
    count: u32,
    /// When each notifier last heard of it.
    sent: BTreeMap<NotifierId, Timestamp>,
    /// Notifiers added or changed while it fired: they do not hear of it.
    skipped: BTreeSet<NotifierId>,
}

/// Acknowledged or silenced: nobody needs telling.
fn attended(alert: &AlertRecord) -> bool {
    alert.acknowledged.is_some() || alert.state.silenced
}

impl Policy {
    pub fn new() -> Self {
        Self::default()
    }

    /// The enabled notifiers and their filters.
    pub fn set_notifiers(
        &mut self,
        notifiers: impl IntoIterator<Item = (NotifierId, NotifierFilter)>,
    ) {
        let notifiers: BTreeMap<_, _> = notifiers.into_iter().collect();
        for tracked in self.alerts.values_mut() {
            tracked.sent.retain(|id, _| notifiers.contains_key(id));
            tracked.skipped.retain(|id| notifiers.contains_key(id));
            for (id, filter) in &notifiers {
                if self.notifiers.get(id) != Some(filter) && !tracked.sent.contains_key(id) {
                    tracked.skipped.insert(*id);
                }
            }
        }
        self.notifiers = notifiers;
    }

    /// The alerts firing at a start: those not attended count as heard of, at `now`, by the
    /// notifiers that admit them.
    pub fn resume(&mut self, firing: Vec<AlertRecord>, now: Timestamp) {
        for alert in firing.into_iter().filter(|a| a.state.active()) {
            let sent = if attended(&alert) {
                BTreeMap::new()
            } else {
                self.admitting(&alert).map(|id| (id, now)).collect()
            };
            self.alerts.insert(
                alert.id,
                Tracked {
                    count: alert.state.count,
                    alert,
                    sent,
                    skipped: BTreeSet::new(),
                },
            );
        }
    }

    fn admitting<'a>(&'a self, alert: &'a AlertRecord) -> impl Iterator<Item = NotifierId> + 'a {
        self.notifiers
            .iter()
            .filter(|(_, filter)| filter.admits(&alert.state))
            .map(|(id, _)| *id)
    }

    /// The alerts as stored (opened, changed, resolved, acknowledged): what to send of them.
    pub fn alerts(&mut self, alerts: &[AlertRecord], now: Timestamp) -> Vec<Send> {
        let mut out = Vec::new();
        for alert in alerts {
            if alert.state.active() {
                self.firing(alert, now, &mut out);
            } else {
                self.resolved(alert, now, &mut out);
            }
        }
        out
    }

    fn resolved(&mut self, alert: &AlertRecord, now: Timestamp, out: &mut Vec<Send>) {
        let Some(tracked) = self.alerts.remove(&alert.id) else {
            return;
        };
        if alert.state.resolution == Some(AlertResolution::RuleChanged) {
            let key = (alert.rule, alert.state.subject.clone());
            self.replaced.insert(key, (now, tracked.sent));
            return;
        }
        for id in tracked.sent.keys() {
            if self.notifiers.get(id).is_some_and(|filter| filter.resolved) {
                out.push(Send {
                    notifier: *id,
                    kind: Kind::Resolved,
                    alert: alert.clone(),
                });
            }
        }
    }

    fn firing(&mut self, alert: &AlertRecord, now: Timestamp, out: &mut Vec<Send>) {
        let state = &alert.state;
        let tracked = self.alerts.entry(alert.id).or_insert_with(|| {
            let carried = self
                .replaced
                .remove(&(alert.rule, state.subject.clone()))
                .filter(|(at, _)| now.saturating_duration_since(*at) <= CARRY_OVER);
            Tracked {
                alert: alert.clone(),
                count: if carried.is_some() { state.count } else { 0 },
                sent: carried.map(|(_, sent)| sent).unwrap_or_default(),
                skipped: BTreeSet::new(),
            }
        });
        let reopened = tracked.count > 0 && state.count > tracked.count;
        tracked.count = state.count;
        tracked.alert = alert.clone();
        if attended(alert) {
            return;
        }
        for (id, filter) in &self.notifiers {
            if tracked.skipped.contains(id) || !filter.admits(state) {
                continue;
            }
            if reopened || !tracked.sent.contains_key(id) {
                let kind = if state.count > 1 {
                    Kind::Reopened
                } else {
                    Kind::Opened
                };
                out.push(Send {
                    notifier: *id,
                    kind,
                    alert: alert.clone(),
                });
                tracked.sent.insert(*id, now);
            }
        }
    }

    /// Reminders due at `now`.
    pub fn tick(&mut self, now: Timestamp) -> Vec<Send> {
        self.replaced
            .retain(|_, (at, _)| now.saturating_duration_since(*at) <= CARRY_OVER);
        let mut out = Vec::new();
        for tracked in self.alerts.values_mut() {
            if attended(&tracked.alert) {
                continue;
            }
            for (id, last) in &mut tracked.sent {
                let Some(every) = self.notifiers.get(id).and_then(NotifierFilter::reminder) else {
                    continue;
                };
                if now.saturating_duration_since(*last) >= every {
                    *last = now;
                    out.push(Send {
                        notifier: *id,
                        kind: Kind::Reminder,
                        alert: tracked.alert.clone(),
                    });
                }
            }
        }
        out
    }

    /// How many alerts it follows.
    pub fn firing_count(&self) -> usize {
        self.alerts.len()
    }
}

#[cfg(test)]
#[path = "policy_tests.rs"]
mod tests;
