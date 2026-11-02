//! Conditions: states that begin and end, such as a node being offline. A bad state raises its
//! condition once it has lasted a while (so a blip raises nothing); a good one clears it at
//! once. Raising and clearing are events that carry the condition's key, and clearing tells
//! how long the condition lasted.

use std::{collections::BTreeMap, time::Duration};

use uniwrt_core::Timestamp;
use uniwrt_model::events::NewEvent;

#[derive(Debug, Default)]
struct State {
    /// When it was raised (when the bad state began).
    raised: Option<Timestamp>,
    /// A bad state not yet long enough: since when, how long it must last, and the event that
    /// raises it (the latest observation's).
    pending: Option<(Timestamp, Duration, NewEvent)>,
}

#[derive(Debug, Default)]
pub(crate) struct Conditions {
    states: BTreeMap<String, State>,
}

impl Conditions {
    /// A condition raised at `at` before the controller started (the store's latest event for
    /// it raised it).
    pub(crate) fn seed(&mut self, key: String, at: Timestamp) {
        self.states.entry(key).or_default().raised = Some(at);
    }

    #[cfg(test)]
    pub(crate) fn is_raised(&self, key: &str) -> bool {
        self.states
            .get(key)
            .is_some_and(|state| state.raised.is_some())
    }

    /// The state is bad at `now`: raises the condition with `event` once it has been bad for
    /// `after` (at once when `after` is zero). The event is stamped with the time the bad state
    /// began.
    pub(crate) fn bad(
        &mut self,
        key: &str,
        now: Timestamp,
        after: Duration,
        event: impl FnOnce() -> NewEvent,
    ) -> Option<NewEvent> {
        let state = self.states.entry(key.to_owned()).or_default();
        if state.raised.is_some() {
            return None;
        }
        let since = state.pending.as_ref().map_or(now, |(since, ..)| *since);
        state.pending = Some((since, after, event()));
        Self::due(key, state, now)
    }

    /// The state is good at `now`: forgets a pending raise, and clears a raised condition with
    /// the event `clear` makes from how long it lasted.
    pub(crate) fn good(
        &mut self,
        key: &str,
        now: Timestamp,
        clear: impl FnOnce(Option<Duration>) -> NewEvent,
    ) -> Option<NewEvent> {
        let state = self.states.get_mut(key)?;
        state.pending = None;
        let raised = state.raised.take()?;
        let lasted = (now >= raised).then(|| now.saturating_duration_since(raised));
        Some(clear(lasted).at_condition(key, false, now))
    }

    /// Nothing is known about it now (its node is offline, its link gone): forgets a pending
    /// raise, and keeps a raised condition.
    pub(crate) fn unknown(&mut self, key: &str) {
        if let Some(state) = self.states.get_mut(key) {
            state.pending = None;
        }
    }

    /// Forgets every condition whose key starts with `prefix`, without events.
    pub(crate) fn forget(&mut self, prefix: &str) {
        self.states.retain(|key, _| !key.starts_with(prefix));
    }

    /// The conditions whose bad state has now lasted long enough.
    pub(crate) fn tick(&mut self, now: Timestamp) -> Vec<NewEvent> {
        let mut raised = Vec::new();
        for (key, state) in &mut self.states {
            raised.extend(Self::due(key, state, now));
        }
        self.states
            .retain(|_, state| state.raised.is_some() || state.pending.is_some());
        raised
    }

    fn due(key: &str, state: &mut State, now: Timestamp) -> Option<NewEvent> {
        let (since, after, _) = state.pending.as_ref()?;
        if now.saturating_duration_since(*since) < *after {
            return None;
        }
        let (since, _, event) = state.pending.take()?;
        state.raised = Some(since);
        Some(event.at_condition(key, true, since))
    }
}

trait AtCondition {
    fn at_condition(self, key: &str, raised: bool, at: Timestamp) -> Self;
}

impl AtCondition for NewEvent {
    fn at_condition(mut self, key: &str, raised: bool, at: Timestamp) -> Self {
        self.at = at;
        self.last_at = at;
        self.condition(key, raised)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use uniwrt_core::Timestamp;
    use uniwrt_model::events::{EventKind, NewEvent};

    use super::Conditions;

    const fn at(s: i64) -> Timestamp {
        Timestamp::from_unix_ms(s * 1_000)
    }

    fn down() -> NewEvent {
        NewEvent::new(
            at(0),
            EventKind::WanDown {
                device: "eth1".into(),
            },
        )
    }

    fn up(lasted: Option<Duration>) -> NewEvent {
        let down_ms = lasted.map(|d| u64::try_from(d.as_millis()).unwrap());
        NewEvent::new(
            at(0),
            EventKind::WanUp {
                device: "eth1".into(),
                down_ms,
            },
        )
    }

    #[test]
    fn raises_once_the_bad_state_lasts_and_clears_with_how_long() {
        let mut conditions = Conditions::default();
        let after = Duration::from_secs(10);
        assert_eq!(conditions.bad("wan", at(100), after, down), None);
        assert_eq!(conditions.bad("wan", at(105), after, down), None);
        assert_eq!(conditions.tick(at(109)), Vec::new());
        let raised = conditions.tick(at(110));
        assert_eq!(raised.len(), 1);
        assert_eq!(raised[0].at, at(100), "stamped when it began");
        assert!(raised[0].condition.as_ref().unwrap().raised);
        assert_eq!(
            conditions.bad("wan", at(111), after, down),
            None,
            "raised once"
        );
        let cleared = conditions.good("wan", at(160), up).unwrap();
        assert_eq!(cleared.at, at(160));
        assert_eq!(
            cleared.what,
            EventKind::WanUp {
                device: "eth1".into(),
                down_ms: Some(60_000)
            }
        );
        assert!(!cleared.condition.unwrap().raised);
        assert_eq!(conditions.good("wan", at(161), up), None);
    }

    #[test]
    fn a_blip_raises_nothing() {
        let mut conditions = Conditions::default();
        let after = Duration::from_secs(10);
        conditions.bad("wan", at(100), after, down);
        assert_eq!(conditions.good("wan", at(103), up), None);
        assert_eq!(conditions.tick(at(200)), Vec::new());
        conditions.bad("wan", at(300), after, down);
        conditions.unknown("wan");
        assert_eq!(conditions.tick(at(400)), Vec::new());
    }

    #[test]
    fn raises_at_once_without_a_delay_and_resumes_seeded_ones() {
        let mut conditions = Conditions::default();
        assert!(
            conditions
                .bad("flash", at(5), Duration::ZERO, down)
                .is_some()
        );
        conditions.seed("wan".into(), at(50));
        assert!(conditions.is_raised("wan"));
        assert_eq!(conditions.bad("wan", at(60), Duration::ZERO, down), None);
        assert!(conditions.good("wan", at(80), up).is_some());
        conditions.forget("fl");
        assert!(!conditions.is_raised("flash"));
    }
}
