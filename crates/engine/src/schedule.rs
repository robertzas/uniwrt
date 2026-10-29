//! How often each collect set is collected ([`Intervals`]), what that means for one agent
//! ([`Plan`]: the sets it knows, at the node's current focus), and the [`Poller`] that collects
//! on that plan when the agent cannot push ticks.

use std::{collections::BTreeMap, time::Duration};

use tokio::time::Instant;
use uniwrt_node::proto::{
    AgentCaps,
    ops::{Set, SetInterval},
};

/// Sets due this soon after the earliest one are collected with it, in one request.
const COALESCE: Duration = Duration::from_millis(500);
/// Telemetry is late after three of its fastest intervals, but never sooner than this.
const MIN_STALE_AFTER: Duration = Duration::from_secs(30);

/// How often each collect set is collected (features/observability.md, Telemetry pipeline).
/// A set without an interval is collected only when the agent starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Intervals {
    pub base: BTreeMap<Set, Duration>,
    /// Overrides while the UI has the node open ([`Engine::focus`](crate::Engine::focus)).
    pub focused: BTreeMap<Set, Duration>,
}

impl Default for Intervals {
    /// 15 s for what changes all the time and 2 s for it while focused; longer for what
    /// changes slowly or is costly to collect (D-037). `board` changes only with the firmware,
    /// which restarts the agent.
    fn default() -> Self {
        let secs = Duration::from_secs;
        Self {
            base: BTreeMap::from([
                (Set::System, secs(15)),
                (Set::Devices, secs(15)),
                (Set::Interfaces, secs(15)),
                (Set::Radios, secs(15)),
                (Set::Stations, secs(15)),
                (Set::Neighbors, secs(30)),
                (Set::Batman, secs(30)),
                (Set::Leases, secs(60)),
                (Set::Capabilities, secs(3600)),
                (Set::Phys, secs(3600)),
            ]),
            focused: BTreeMap::from([
                (Set::System, secs(2)),
                (Set::Devices, secs(2)),
                (Set::Radios, secs(2)),
                (Set::Stations, secs(2)),
                (Set::Batman, secs(10)),
            ]),
        }
    }
}

impl Intervals {
    /// The intervals of the sets `caps` knows, within what the agent accepts.
    pub(crate) fn plan(&self, focused: bool, caps: &AgentCaps) -> Plan {
        let min = Duration::from_millis(SetInterval::MIN_MS);
        let max = Duration::from_millis(SetInterval::MAX_MS);
        let mut plan = self.base.clone();
        if focused {
            plan.extend(&self.focused);
        }
        plan.retain(|&set, _| caps.has_set(set));
        for interval in plan.values_mut() {
            *interval = (*interval).clamp(min, max);
        }
        Plan(plan)
    }
}

/// Every scheduled set's interval, for one agent.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Plan(BTreeMap<Set, Duration>);

impl Plan {
    /// The sets of each interval, as `set_interval` takes them.
    pub(crate) fn requests(&self) -> Vec<SetInterval> {
        let mut groups = BTreeMap::<Duration, Vec<Set>>::new();
        for (&set, &interval) in &self.0 {
            groups.entry(interval).or_default().push(set);
        }
        groups
            .into_iter()
            .map(|(interval, sets)| SetInterval {
                interval_ms: millis(interval),
                sets,
            })
            .collect()
    }

    /// How long telemetry may be missing before it is late; `None` when nothing is scheduled.
    pub(crate) fn stale_after(&self) -> Option<Duration> {
        let fastest = self.0.values().min()?;
        Some((*fastest * 3).max(MIN_STALE_AFTER))
    }
}

/// Collects a [`Plan`] by itself: each set when it is due, sets due together in one request.
#[derive(Debug, Default)]
pub(crate) struct Poller {
    /// Each set's interval and when it is due next.
    sets: BTreeMap<Set, (Duration, Instant)>,
}

impl Poller {
    /// Every set is first due one interval from `now`: the agent's start collected them all.
    pub(crate) fn new(plan: &Plan, now: Instant) -> Self {
        let sets = plan
            .0
            .iter()
            .map(|(&set, &interval)| (set, (interval, now + interval)))
            .collect();
        Self { sets }
    }

    /// Moves to a new plan. A set whose interval shrank is due no later than one new interval
    /// from `now`, so focusing a node speeds it up at once; one whose interval grew keeps its
    /// next time.
    pub(crate) fn replan(&mut self, plan: &Plan, now: Instant) {
        self.sets = plan
            .0
            .iter()
            .map(|(&set, &interval)| {
                let due = match self.sets.get(&set) {
                    Some(&(_, due)) => due.min(now + interval),
                    None => now + interval,
                };
                (set, (interval, due))
            })
            .collect();
    }

    pub(crate) fn next_due(&self) -> Option<Instant> {
        self.sets.values().map(|&(_, due)| due).min()
    }

    /// The sets due by `now` (or about to be), each rescheduled one interval on; a set that fell
    /// more than an interval behind is due one interval from `now`.
    pub(crate) fn take_due(&mut self, now: Instant) -> Vec<Set> {
        let mut due = Vec::new();
        for (&set, (interval, next)) in &mut self.sets {
            if *next <= now + COALESCE {
                due.push(set);
                *next += *interval;
                if *next <= now {
                    *next = now + *interval;
                }
            }
        }
        due
    }
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::time::Instant;
    use uniwrt_node::proto::{AgentCaps, ops::Set};

    use super::{Intervals, Plan, Poller};

    fn caps(sets: &[Set]) -> AgentCaps {
        AgentCaps {
            events: true,
            sets: sets.iter().map(|set| set.as_str().to_owned()).collect(),
            ..AgentCaps::default()
        }
    }

    fn secs(secs: u64) -> Duration {
        Duration::from_secs(secs)
    }

    #[test]
    fn plans_keep_the_sets_the_agent_knows_and_group_them_by_interval() {
        let caps = caps(&[Set::System, Set::Stations, Set::Leases, Set::Board]);
        let plan = Intervals::default().plan(false, &caps);
        let groups: Vec<_> = plan
            .requests()
            .into_iter()
            .map(|r| (r.interval_ms, r.sets))
            .collect();
        assert_eq!(
            groups,
            [
                (15_000, vec![Set::Stations, Set::System]),
                (60_000, vec![Set::Leases])
            ]
        );
        assert_eq!(plan.stale_after(), Some(secs(45)));

        let focused = Intervals::default().plan(true, &caps);
        let groups: Vec<_> = focused
            .requests()
            .into_iter()
            .map(|r| (r.interval_ms, r.sets))
            .collect();
        assert_eq!(
            groups,
            [
                (2_000, vec![Set::Stations, Set::System]),
                (60_000, vec![Set::Leases])
            ]
        );
        assert_eq!(focused.stale_after(), Some(secs(30)));
        assert_eq!(Plan::default().stale_after(), None);
    }

    #[test]
    fn intervals_are_clamped_to_what_the_agent_accepts() {
        let intervals = Intervals {
            base: [(Set::System, Duration::from_millis(10))].into(),
            focused: [].into(),
        };
        let plan = intervals.plan(false, &caps(&[Set::System]));
        assert_eq!(plan.requests()[0].interval_ms, 1_000);
    }

    #[tokio::test(start_paused = true)]
    async fn the_poller_coalesces_due_sets_and_catches_up_without_bursts() {
        let caps = caps(&[Set::System, Set::Leases, Set::Neighbors]);
        let plan = Intervals::default().plan(false, &caps);
        let start = Instant::now();
        let mut poller = Poller::new(&plan, start);
        assert_eq!(poller.next_due(), Some(start + secs(15)));
        assert_eq!(poller.take_due(start + secs(14)), []);
        assert_eq!(poller.take_due(start + secs(15)), [Set::System]);
        assert_eq!(
            poller.take_due(start + secs(30)),
            [Set::Neighbors, Set::System]
        );
        // Fell behind by minutes: each set comes back one interval from now, not in a burst.
        let late = start + secs(300);
        assert_eq!(
            poller.take_due(late),
            [Set::Leases, Set::Neighbors, Set::System]
        );
        assert_eq!(poller.next_due(), Some(late + secs(15)));
    }

    #[tokio::test(start_paused = true)]
    async fn focusing_speeds_the_poller_up_at_once_and_unfocusing_keeps_its_times() {
        let caps = caps(&[Set::System, Set::Leases]);
        let intervals = Intervals::default();
        let start = Instant::now();
        let mut poller = Poller::new(&intervals.plan(false, &caps), start);
        poller.replan(&intervals.plan(true, &caps), start + secs(1));
        assert_eq!(poller.next_due(), Some(start + secs(3)));
        assert_eq!(poller.take_due(start + secs(3)), [Set::System]);
        poller.replan(&intervals.plan(false, &caps), start + secs(3));
        assert_eq!(poller.next_due(), Some(start + secs(5)));
        assert_eq!(poller.take_due(start + secs(5)), [Set::System]);
        assert_eq!(poller.next_due(), Some(start + secs(20)));
    }
}
