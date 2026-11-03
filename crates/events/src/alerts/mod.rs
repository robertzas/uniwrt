//! Alerts (features/observability.md, Alerts): rules over the windows the metrics sampler
//! closes and the events the recorder stores, firing one alert per rule and subject until it
//! resolves.
//!
//! - **Metric rules:** a series beyond its threshold for the rule's `for_s` (contiguous windows;
//!   a missing window is tolerated, two start over) fires; it resolves once a window is back on
//!   the safe side of `clear` (hysteresis), or when the series has said nothing for
//!   [`AlertsConfig::stale_after`].
//! - **Event rules:** `count` matches within `window_s` fire, per subject: the node, else the
//!   client, else the site. With a count of 1, an event that raises a condition is its own
//!   subject (the condition's key) and its alert resolves when the condition clears; other alerts
//!   resolve once the window holds fewer matches. Folded repeats count as matches.
//! - **Dedup and cool-down:** one alert per rule and subject while it fires; firing again within
//!   the rule's cool-down of resolving reopens that alert (its count grows).
//! - **Silencing:** an alert that fires during maintenance ([`Silences`]: a job on its node and
//!   [`AlertsConfig::after_job`] after, or a maintenance window) is silenced; it stops being so
//!   once maintenance ends while it still fires.
//! - **Rules changing:** alerts of a rule that is deleted, disabled or changed resolve
//!   ([`AlertResolution::RuleChanged`]) and start afresh.

mod event;
mod metric;
mod presets;
#[cfg(test)]
mod tests;

use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    time::Duration,
};

use uniwrt_core::{EventId, MacAddr, NodeId, RuleId, Timestamp};
use uniwrt_model::{
    alerts::{
        AlertCause, AlertRecord, AlertResolution, AlertRule, AlertState, AlertWrite,
        MaintenanceWindow, RuleCondition,
    },
    events::EventRecord,
    metric::{Metric, Sample},
};

pub use self::presets::{Preset, PresetError, presets};

#[derive(Debug, Clone, Copy)]
pub struct AlertsConfig {
    /// The metrics' window.
    pub window: Duration,
    /// A metric alert whose series says nothing for this long resolves.
    pub stale_after: Duration,
    /// How long after a job on a node its alerts stay silenced.
    pub after_job: Duration,
}

impl Default for AlertsConfig {
    fn default() -> Self {
        Self {
            window: Duration::from_secs(15),
            stale_after: Duration::from_mins(5),
            after_job: Duration::from_mins(5),
        }
    }
}

/// An alert's rule and subject.
type Key = (RuleId, String);

/// The rules and the alerts they fire.
#[derive(Debug)]
pub struct Evaluator {
    config: AlertsConfig,
    rules: BTreeMap<RuleId, AlertRule>,
    book: Book,
}

/// What the evaluator tracks besides its rules.
#[derive(Debug, Default)]
struct Book {
    /// Alerts firing, and resolved ones still within their cool-down.
    alerts: HashMap<Key, Tracked>,
    /// Series beyond a threshold, not firing yet: since when, and the end of the latest window.
    pending: HashMap<Key, (Timestamp, Timestamp)>,
    /// Event rules' matches within their windows: when, and how many.
    matches: HashMap<Key, VecDeque<(Timestamp, u32)>>,
    /// Events counted, with their count then and their last repeat.
    seen: HashMap<EventId, (u32, Timestamp)>,
    silences: Silences,
}

#[derive(Debug)]
struct Tracked {
    state: AlertState,
    /// A metric alert's latest window that kept it firing.
    seen_at: Timestamp,
}

impl Evaluator {
    /// Resumes the alerts the store holds: those firing, and the latest resolved within their
    /// cool-down. [`Evaluator::set_rules`] comes next.
    pub fn new(config: AlertsConfig, restored: Vec<AlertRecord>, now: Timestamp) -> Self {
        let mut book = Book::default();
        book.silences.after_job = config.after_job;
        for alert in restored {
            let key = (alert.rule, alert.state.subject.clone());
            if alert.state.active()
                && let AlertCause::Event {
                    matches,
                    condition: None,
                    ..
                } = alert.state.cause
            {
                book.matches.insert(
                    key.clone(),
                    VecDeque::from([(alert.state.last_at, matches)]),
                );
            }
            book.alerts.insert(
                key,
                Tracked {
                    state: alert.state,
                    seen_at: now,
                },
            );
        }
        Self {
            config,
            rules: BTreeMap::new(),
            book,
        }
    }

    pub fn silences(&mut self) -> &mut Silences {
        &mut self.book.silences
    }

    /// Replaces the rules: alerts of rules deleted, disabled or changed resolve.
    pub fn set_rules(&mut self, rules: Vec<AlertRule>, now: Timestamp) -> Vec<AlertWrite> {
        let old = std::mem::take(&mut self.rules);
        self.rules = rules.into_iter().map(|rule| (rule.id, rule)).collect();
        let stale = |id: &RuleId| match self.rules.get(id) {
            None => true,
            Some(rule) => {
                !rule.spec.enabled || old.get(id).is_some_and(|old| old.spec != rule.spec)
            }
        };
        let book = &mut self.book;
        book.pending.retain(|(id, _), _| !stale(id));
        book.matches.retain(|(id, _), _| !stale(id));
        let mut out = Vec::new();
        book.alerts.retain(|(id, _), tracked| {
            if !stale(id) {
                return true;
            }
            if tracked.state.active() {
                resolve(&mut tracked.state, now, AlertResolution::RuleChanged);
                out.push(AlertWrite::Update {
                    rule: *id,
                    state: tracked.state.clone(),
                });
            }
            false
        });
        out
    }

    /// Windows the sampler closed.
    pub fn windows(&mut self, samples: &[Sample]) -> Vec<AlertWrite> {
        let mut by_metric: HashMap<Metric, Vec<&AlertRule>> = HashMap::new();
        for rule in self.rules.values().filter(|rule| rule.spec.enabled) {
            if let RuleCondition::Metric(condition) = &rule.spec.when {
                by_metric.entry(condition.metric).or_default().push(rule);
            }
        }
        let mut out = Vec::new();
        if by_metric.is_empty() {
            return out;
        }
        let mut ordered: Vec<&Sample> = samples.iter().collect();
        ordered.sort_by_key(|sample| sample.at);
        for sample in ordered {
            for rule in by_metric.get(&sample.metric).into_iter().flatten() {
                if let RuleCondition::Metric(condition) = &rule.spec.when
                    && condition.entities.matches(&sample.entity)
                {
                    self.book
                        .sample(rule, condition, sample, self.config.window, &mut out);
                }
            }
        }
        out
    }

    /// Events as recorded (new ones, and repeats folded into them), in order.
    pub fn events(&mut self, records: &[EventRecord], now: Timestamp) -> Vec<AlertWrite> {
        let rules: Vec<&AlertRule> = self
            .rules
            .values()
            .filter(|rule| rule.spec.enabled && matches!(rule.spec.when, RuleCondition::Event(_)))
            .collect();
        let mut out = Vec::new();
        for record in records {
            self.book.record(&rules, record, now, &mut out);
        }
        out
    }

    /// Resolves what went quiet, lifts silences that ended, and forgets what is past.
    pub fn tick(&mut self, now: Timestamp) -> Vec<AlertWrite> {
        let mut out = Vec::new();
        let config = self.config;
        let book = &mut self.book;
        for ((id, subject), tracked) in &mut book.alerts {
            let state = &mut tracked.state;
            if !state.active() {
                continue;
            }
            let rule = self.rules.get(id);
            let mut changed = false;
            match (&state.cause, rule.map(|rule| &rule.spec.when)) {
                (AlertCause::Metric { .. }, _)
                    if tracked.seen_at.saturating_add(config.stale_after) <= now =>
                {
                    resolve(state, now, AlertResolution::NoData);
                    changed = true;
                }
                (
                    AlertCause::Event {
                        condition: None, ..
                    },
                    Some(RuleCondition::Event(condition)),
                ) => {
                    let key = (*id, subject.clone());
                    let window = Duration::from_secs(condition.window_s.into());
                    let total = book.matches.get_mut(&key).map_or(0, |matches| {
                        prune(matches, now.saturating_sub(window));
                        matches.iter().map(|(_, count)| count).sum()
                    });
                    if total < condition.count {
                        resolve(state, now, AlertResolution::Cleared);
                        changed = true;
                    }
                }
                _ => {}
            }
            if state.active() && state.silenced && !book.silences.silenced(state.node, now) {
                state.silenced = false;
                changed = true;
            }
            if changed {
                out.push(AlertWrite::Update {
                    rule: *id,
                    state: state.clone(),
                });
            }
        }
        self.forget(now);
        out
    }

    fn forget(&mut self, now: Timestamp) {
        let rules = &self.rules;
        let cooldown = |id: &RuleId| {
            rules.get(id).map_or(Duration::ZERO, |rule| {
                Duration::from_secs(rule.spec.cooldown_s.into())
            })
        };
        let window = |id: &RuleId| match rules.get(id).map(|rule| &rule.spec.when) {
            Some(RuleCondition::Event(condition)) => Duration::from_secs(condition.window_s.into()),
            _ => Duration::ZERO,
        };
        let book = &mut self.book;
        book.alerts.retain(|(id, _), tracked| {
            tracked
                .state
                .resolved_at
                .is_none_or(|at| at.saturating_add(cooldown(id)) > now)
        });
        let gap = self.config.window.saturating_mul(2);
        book.pending
            .retain(|_, (_, until)| until.saturating_add(gap) >= now);
        book.matches.retain(|(id, _), matches| {
            prune(matches, now.saturating_sub(window(id)));
            !matches.is_empty()
        });
        // Repeats fold into an event for 5 minutes after its last; keep its count longer.
        let longest = rules
            .keys()
            .map(window)
            .max()
            .unwrap_or_default()
            .max(Duration::from_mins(10));
        book.seen
            .retain(|_, (_, last)| last.saturating_add(longest) >= now);
        book.silences.forget(now);
    }
}

impl Book {
    /// `rule` fired for `subject` at `at`: a new alert, or the last one reopened within the
    /// rule's cool-down.
    #[allow(clippy::too_many_arguments)]
    fn fire(
        &mut self,
        rule: &AlertRule,
        subject: String,
        node: Option<NodeId>,
        client: Option<MacAddr>,
        at: Timestamp,
        cause: AlertCause,
        out: &mut Vec<AlertWrite>,
    ) {
        let silenced = self.silences.silenced(node, at);
        let cooldown = Duration::from_secs(rule.spec.cooldown_s.into());
        let key = (rule.id, subject);
        if let Some(tracked) = self.alerts.get_mut(&key)
            && tracked
                .state
                .resolved_at
                .is_some_and(|resolved| at < resolved.saturating_add(cooldown))
        {
            let state = &mut tracked.state;
            state.resolved_at = None;
            state.resolution = None;
            state.count = state.count.saturating_add(1);
            state.last_at = at;
            state.level = rule.spec.level;
            state.name.clone_from(&rule.spec.name);
            state.silenced = silenced;
            state.cause = cause;
            tracked.seen_at = at;
            out.push(AlertWrite::Reopen {
                rule: rule.id,
                state: state.clone(),
            });
            return;
        }
        let state = AlertState {
            name: rule.spec.name.clone(),
            level: rule.spec.level,
            subject: key.1.clone(),
            node,
            client,
            started_at: at,
            last_at: at,
            resolved_at: None,
            resolution: None,
            count: 1,
            silenced,
            cause,
        };
        out.push(AlertWrite::Open {
            rule: rule.id,
            state: state.clone(),
        });
        self.alerts.insert(key, Tracked { state, seen_at: at });
    }

    fn active(&mut self, key: &Key) -> Option<&mut Tracked> {
        self.alerts
            .get_mut(key)
            .filter(|tracked| tracked.state.active())
    }
}

fn resolve(state: &mut AlertState, at: Timestamp, resolution: AlertResolution) {
    state.resolved_at = Some(at.max(state.started_at));
    state.resolution = Some(resolution);
}

/// Drops the matches at or before `since`: a match a whole window old is out of it.
fn prune(matches: &mut VecDeque<(Timestamp, u32)>, since: Timestamp) {
    while matches.front().is_some_and(|(at, _)| *at <= since) {
        matches.pop_front();
    }
}

/// Maintenance: jobs running on nodes, and maintenance windows.
#[derive(Debug, Default)]
pub struct Silences {
    /// Nodes with a job: `None` while it runs, else when it finished.
    jobs: HashMap<NodeId, Option<Timestamp>>,
    windows: Vec<MaintenanceWindow>,
    after_job: Duration,
}

impl Silences {
    /// A job on `node` started (`running`) or finished at `at`.
    pub fn job(&mut self, node: NodeId, running: bool, at: Timestamp) {
        self.jobs.insert(node, (!running).then_some(at));
    }

    pub fn set_windows(&mut self, windows: Vec<MaintenanceWindow>) {
        self.windows = windows;
    }

    /// Whether alerts about `node` are silenced at `at`.
    pub fn silenced(&self, node: Option<NodeId>, at: Timestamp) -> bool {
        let job = node
            .and_then(|node| self.jobs.get(&node))
            .is_some_and(|finished| {
                finished.is_none_or(|finished| at < finished.saturating_add(self.after_job))
            });
        job || self.windows.iter().any(|window| window.covers(node, at))
    }

    fn forget(&mut self, now: Timestamp) {
        let after = self.after_job;
        self.jobs.retain(|_, finished| {
            finished.is_none_or(|finished| finished.saturating_add(after) > now)
        });
        self.windows.retain(|window| window.ends_at > now);
    }
}
