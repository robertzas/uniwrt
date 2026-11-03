//! Metric rules over the sampler's windows: a series beyond its threshold for long enough fires,
//! and resolves back on the safe side of its clear value.

use std::time::Duration;

use uniwrt_model::{
    alerts::{AlertCause, AlertResolution, AlertRule, AlertWrite, MetricCondition},
    metric::{Entity, Sample},
};

use super::{Book, resolve};

impl Book {
    /// One window of a series that `rule` watches.
    pub(super) fn sample(
        &mut self,
        rule: &AlertRule,
        condition: &MetricCondition,
        sample: &Sample,
        window: Duration,
        out: &mut Vec<AlertWrite>,
    ) {
        let key = (rule.id, sample.entity.to_string());
        let end = sample.at.saturating_add(window);
        let value = sample.value;
        let op = condition.op;
        if let Some(tracked) = self.active(&key) {
            let state = &mut tracked.state;
            if op.beyond(value, condition.clear_at()) {
                tracked.seen_at = end;
                state.last_at = end;
                if let AlertCause::Metric { worst, .. } = &mut state.cause
                    && op.beyond(value, *worst)
                {
                    *worst = value;
                    out.push(AlertWrite::Update {
                        rule: rule.id,
                        state: state.clone(),
                    });
                }
            } else {
                resolve(state, sample.at, AlertResolution::Cleared);
                out.push(AlertWrite::Update {
                    rule: rule.id,
                    state: state.clone(),
                });
            }
            return;
        }
        if !op.beyond(value, condition.threshold) {
            self.pending.remove(&key);
            return;
        }
        let (since, until) = self
            .pending
            .entry(key.clone())
            .or_insert((sample.at, sample.at));
        // One missing window is tolerated; a longer gap starts over.
        if sample.at > until.saturating_add(window) {
            *since = sample.at;
        }
        *until = end;
        if end.saturating_duration_since(*since) < Duration::from_secs(condition.for_s.into()) {
            return;
        }
        self.pending.remove(&key);
        let (node, client) = match &sample.entity {
            Entity::Client(mac) => (None, Some(*mac)),
            entity => (entity.node(), None),
        };
        let cause = AlertCause::Metric {
            metric: condition.metric,
            op,
            threshold: condition.threshold,
            value,
            worst: value,
        };
        self.fire(rule, key.1, node, client, end, cause, out);
    }
}
