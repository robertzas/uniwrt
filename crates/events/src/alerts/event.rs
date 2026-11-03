//! Event rules over recorded events: matches counted per subject within the rule's window, and
//! the conditions that clear the alerts they raised.

use std::time::Duration;

use uniwrt_core::Timestamp;
use uniwrt_model::{
    alerts::{AlertCause, AlertResolution, AlertRule, AlertWrite, EventCondition, RuleCondition},
    events::{EventKind, EventRecord},
};

use super::{Book, prune, resolve};

impl Book {
    /// One event, new or with more repeats, against the enabled event `rules`.
    pub(super) fn record(
        &mut self,
        rules: &[&AlertRule],
        record: &EventRecord,
        now: Timestamp,
        out: &mut Vec<AlertWrite>,
    ) {
        if let Some(condition) = record.condition.as_ref().filter(|c| !c.raised) {
            for ((rule, _), tracked) in &mut self.alerts {
                let state = &mut tracked.state;
                if state.active()
                    && matches!(&state.cause, AlertCause::Event { condition: Some(key), .. }
                        if *key == condition.key)
                {
                    resolve(state, record.at, AlertResolution::Cleared);
                    out.push(AlertWrite::Update {
                        rule: *rule,
                        state: state.clone(),
                    });
                }
            }
        }
        let before = self
            .seen
            .insert(record.id, (record.count, record.last_at))
            .map_or(0, |(count, _)| count);
        let added = record.count.saturating_sub(before);
        if added == 0 {
            return;
        }
        for rule in rules {
            let RuleCondition::Event(condition) = &rule.spec.when else {
                continue;
            };
            if !admits(condition, record) {
                continue;
            }
            let raised = record
                .condition
                .as_ref()
                .filter(|c| c.raised && condition.count == 1)
                .map(|c| c.key.clone());
            let subject = raised.clone().unwrap_or_else(|| subject(record));
            let key = (rule.id, subject);
            let window = Duration::from_secs(condition.window_s.into());
            let matches = self.matches.entry(key.clone()).or_default();
            matches.push_back((record.last_at, added));
            prune(matches, now.saturating_sub(window));
            let total: u32 = matches.iter().map(|(_, count)| count).sum();
            if total < condition.count {
                continue;
            }
            let cause = AlertCause::Event {
                event: record.id,
                kind: record.what.name().to_owned(),
                matches: total,
                condition: raised,
            };
            if let Some(tracked) = self.active(&key) {
                tracked.state.last_at = record.last_at;
                tracked.state.cause = cause;
                out.push(AlertWrite::Update {
                    rule: rule.id,
                    state: tracked.state.clone(),
                });
            } else {
                self.fire(
                    rule,
                    key.1,
                    record.node,
                    record.client,
                    record.last_at,
                    cause,
                    out,
                );
            }
        }
    }
}

/// What an event is about: its node, else its client, else the site.
fn subject(record: &EventRecord) -> String {
    match (record.node, record.client) {
        (Some(node), _) => format!("node:{node}"),
        (None, Some(mac)) => format!("client:{mac}"),
        (None, None) => "site".to_owned(),
    }
}

fn admits(condition: &EventCondition, record: &EventRecord) -> bool {
    let name = record.what.name();
    (condition.kinds.is_empty() || condition.kinds.iter().any(|kind| kind == name))
        && (condition.categories.is_empty() || condition.categories.contains(&record.category))
        && condition.min.is_none_or(|min| record.level <= min)
        && (condition.nodes.is_empty()
            || record
                .node
                .is_some_and(|node| condition.nodes.contains(&node)))
        && (condition.patterns.is_empty()
            || matches!(&record.what, EventKind::LogPattern { pattern, .. }
                if condition.patterns.contains(pattern)))
}
