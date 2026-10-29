//! [`NodeActor`]: one task per node. It follows the node's link (its state and the agent's
//! pushes), keeps the node's live facts and status, asks the agent for telemetry on the node's
//! plan, and announces what changes on the bus.
//!
//! After each hello it asks the agent again for what pushes ended with the agent before:
//! topics, then a collect of every set, then the ticks of its plan and, while someone follows
//! the log, the log. An agent without an event loop is polled instead ([`Poller`]), and so is
//! one that refused the ticks. Telemetry that stays away for three of its fastest intervals
//! makes the node degraded and asks the agent for its pushes again.

mod jobs;
mod telemetry;
#[cfg(test)]
mod tests;

use std::{collections::BTreeSet, sync::Arc};

use tokio::{
    sync::{broadcast, watch},
    time::{Duration, Instant, sleep_until},
};
use tracing::{info, warn};
use uniwrt_core::NodeId;
use uniwrt_model::{facts::NodeFacts, log::LogEntry};
use uniwrt_node::{
    AgentEvent, AgentState, Error, Hello, LinkState, NodeLink,
    proto::{
        AgentCaps,
        ops::{LogFollow, LogMode, Set, Topic},
    },
};

use self::jobs::{Done, Job, Jobs, Kind, Outcome};
use crate::{
    bus::Event,
    engine::Context,
    interest::Interests,
    schedule::{Plan, Poller},
    status::{self, Health, Tracker},
};

/// How long after a failed `log_follow` it is tried again.
const LOG_RETRY: Duration = Duration::from_secs(30);
/// How long after a follow ended on the node it starts again.
const LOG_RESTART: Duration = Duration::from_secs(5);

pub(crate) struct NodeActor<L> {
    node: NodeId,
    link: Arc<L>,
    ctx: Arc<Context>,
    link_state: LinkState,
    states: watch::Receiver<LinkState>,
    pushes: Option<broadcast::Receiver<AgentEvent>>,
    interests: watch::Receiver<Interests>,
    /// Counts the agents seen, so a job's outcome is matched to the agent it was sent to.
    generation: u64,
    session: Option<Session>,
    jobs: Jobs,
    facts: Option<Arc<NodeFacts>>,
    health: Health,
    status: Tracker,
    /// Sets that pushed events touched, collected again at `refresh_at`.
    refresh: BTreeSet<Set>,
    refresh_at: Option<Instant>,
    /// When the stale timer last started: the agent's start, the latest telemetry, or the
    /// latest time it ran out.
    stale_from: Instant,
    logs: Logs,
}

/// The running agent.
struct Session {
    caps: AgentCaps,
    focused: bool,
    plan: Plan,
    /// The plan the agent ticks on, once it said so.
    scheduled: Option<Plan>,
    /// Polls the plan when the agent does not tick.
    poller: Option<Poller>,
}

#[derive(Default)]
struct Logs {
    /// The agent follows the log, by its last answer.
    following: bool,
    retry_at: Option<Instant>,
    /// The last entry announced.
    last: Option<LogEntry>,
    /// The next batch may start with entries up to this one, announced before the follow
    /// started again: they are skipped.
    resume: Option<LogEntry>,
}

impl<L: NodeLink> NodeActor<L> {
    pub(crate) fn new(
        ctx: Arc<Context>,
        node: NodeId,
        link: Arc<L>,
        interests: watch::Receiver<Interests>,
    ) -> Self {
        let states = link.state();
        let link_state = states.borrow().clone();
        Self {
            node,
            pushes: Some(link.events()),
            facts: ctx.live.get(node).and_then(|view| view.facts),
            link_state,
            states,
            link,
            ctx,
            interests,
            generation: 0,
            session: None,
            jobs: Jobs::default(),
            health: Health::default(),
            status: Tracker::default(),
            refresh: BTreeSet::new(),
            refresh_at: None,
            stale_from: Instant::now(),
            logs: Logs::default(),
        }
    }

    /// Runs until the link is gone.
    pub(crate) async fn run(mut self) {
        let mut interests_open = true;
        self.on_link_state();
        loop {
            self.reconcile();
            self.jobs
                .start(&self.link, self.generation, self.ctx.config.request_timeout);
            self.publish_status();
            let wake = self.next_wake();
            tokio::select! {
                changed = self.states.changed() => match changed {
                    Ok(()) => self.on_link_state(),
                    Err(_) => return,
                },
                push = recv(&mut self.pushes) => self.on_push(push),
                changed = self.interests.changed(), if interests_open => match changed {
                    Ok(()) => self.on_interests(),
                    Err(_) => interests_open = false,
                },
                outcome = self.jobs.next() => self.on_outcome(outcome),
                () = sleep_until(wake.unwrap_or_else(Instant::now)), if wake.is_some() => {
                    self.on_timer();
                }
            }
        }
    }

    fn on_link_state(&mut self) {
        self.link_state = self.states.borrow_and_update().clone();
        let hello = match &self.link_state {
            LinkState::Up {
                agent: AgentState::Running(hello),
                ..
            } => Some(hello.clone()),
            _ => None,
        };
        match (hello, self.session.is_some()) {
            (Some(hello), false) => self.start_session(&hello),
            (None, true) => self.end_session(),
            _ => {}
        }
    }

    fn start_session(&mut self, hello: &Hello) {
        self.generation += 1;
        self.jobs.clear();
        self.health.reset();
        let caps = hello.caps.clone();
        let focused = self.interests.borrow().focus > 0;
        let plan = self.ctx.config.intervals.plan(focused, &caps);
        let now = Instant::now();
        let poller = if caps.events {
            let topics: Vec<_> = Topic::ALL
                .into_iter()
                .filter(|&topic| caps.has_topic(topic))
                .collect();
            if !topics.is_empty() {
                self.jobs.push(Job::Subscribe(topics));
            }
            None
        } else {
            Some(Poller::new(&plan, now))
        };
        let sets: Vec<_> = Set::ALL
            .into_iter()
            .filter(|&set| caps.has_set(set))
            .collect();
        if !sets.is_empty() {
            self.jobs.push(Job::Collect(sets));
        }
        info!(
            release = hello.release.as_deref().unwrap_or("unknown"),
            events = caps.events,
            "the agent runs"
        );
        self.stale_from = now;
        self.logs.following = false;
        self.logs.retry_at = None;
        self.session = Some(Session {
            caps,
            focused,
            plan,
            scheduled: None,
            poller,
        });
    }

    fn end_session(&mut self) {
        self.session = None;
        self.jobs.clear();
        self.refresh.clear();
        self.refresh_at = None;
        self.logs.following = false;
        self.logs.retry_at = None;
    }

    fn on_interests(&mut self) {
        let focused = self.interests.borrow_and_update().focus > 0;
        let Some(session) = &mut self.session else {
            return;
        };
        if focused != session.focused {
            session.focused = focused;
            session.plan = self.ctx.config.intervals.plan(focused, &session.caps);
            if let Some(poller) = &mut session.poller {
                poller.replan(&session.plan, Instant::now());
            }
        }
    }

    /// Queues what the agent should be told: the current plan, and following the log or not.
    fn reconcile(&mut self) {
        let Some(session) = &self.session else {
            return;
        };
        if session.poller.is_none()
            && session.scheduled.as_ref() != Some(&session.plan)
            && !self.jobs.pending(Kind::Schedule)
        {
            self.jobs.push(Job::Schedule(session.plan.clone()));
        }
        let wanted = session.caps.events && self.interests.borrow().logs > 0;
        if wanted != self.logs.following
            && self.logs.retry_at.is_none()
            && !self.jobs.pending(Kind::Logs)
        {
            let follow = if wanted {
                self.logs.resume.clone_from(&self.logs.last);
                LogFollow {
                    backfill: self.ctx.config.log_backfill.min(LogFollow::MAX_BACKFILL),
                    ..LogFollow::new(self.ctx.config.log_mode)
                }
            } else {
                LogFollow::new(LogMode::Off)
            };
            self.jobs.push(Job::Logs(follow));
        }
    }

    fn on_outcome(&mut self, Outcome { generation, done }: Outcome) {
        let current = generation == self.generation && self.session.is_some();
        if let Done::Collected(Ok(collected)) = &done {
            // Facts are facts, whichever agent collected them.
            self.apply(collected);
        }
        if !current {
            return;
        }
        let now = Instant::now();
        let (op, result) = match done {
            Done::Subscribed(result) => ("subscribe", result),
            Done::Collected(result) => ("collect", result.map(drop)),
            Done::Scheduled { plan, result } => {
                if let Some(session) = &mut self.session {
                    if result.is_ok() {
                        session.scheduled = Some(plan);
                    } else if session.poller.is_none() {
                        warn!("the agent does not tick; polling it instead");
                        session.poller = Some(Poller::new(&session.plan, now));
                    }
                }
                ("set_interval", result)
            }
            Done::Logs { mode, result } => {
                if result.is_ok() {
                    self.logs.following = mode != LogMode::Off;
                } else {
                    self.logs.retry_at = Some(now + LOG_RETRY);
                }
                ("log_follow", result)
            }
        };
        match result {
            Ok(()) => {
                self.health.requests.remove(op);
            }
            // The link's state says why.
            Err(Error::Disconnected { .. } | Error::AgentUnavailable { .. }) => {}
            Err(err) => {
                warn!(op, %err, "a request to the agent failed");
                self.health.requests.insert(op, err.to_string());
            }
        }
    }

    fn on_timer(&mut self) {
        let now = Instant::now();
        if self.refresh_at.is_some_and(|at| at <= now) {
            self.refresh_at = None;
            let sets = std::mem::take(&mut self.refresh);
            self.jobs.push(Job::Collect(sets.into_iter().collect()));
        }
        if self.logs.retry_at.is_some_and(|at| at <= now) {
            self.logs.retry_at = None;
        }
        let Some(session) = &mut self.session else {
            return;
        };
        if let Some(poller) = &mut session.poller
            && poller.next_due().is_some_and(|due| due <= now)
        {
            let due = poller.take_due(now);
            self.jobs.push(Job::Collect(due));
        }
        if let Some(after) = session.plan.stale_after()
            && self.stale_from + after <= now
        {
            self.stale_from = now;
            if !self.health.stale {
                warn!(
                    ?after,
                    "no telemetry arrived; asking the agent for it again"
                );
            }
            self.health.stale = true;
            if session.poller.is_none() {
                // The agent may have restarted unseen: its pushes ended with it.
                session.scheduled = None;
                let topics: Vec<_> = Topic::ALL
                    .into_iter()
                    .filter(|&topic| session.caps.has_topic(topic))
                    .collect();
                if !topics.is_empty() {
                    self.jobs.push(Job::Subscribe(topics));
                }
                self.logs.following = false;
            }
        }
    }

    fn next_wake(&self) -> Option<Instant> {
        let session = self.session.as_ref();
        let stale = session
            .and_then(|session| session.plan.stale_after())
            .map(|after| self.stale_from + after);
        let poll = session
            .and_then(|session| session.poller.as_ref())
            .and_then(Poller::next_due);
        [self.refresh_at, self.logs.retry_at, stale, poll]
            .into_iter()
            .flatten()
            .min()
    }

    fn publish_status(&mut self) {
        let now = self.ctx.clock.now();
        let derived = status::derive(&self.link_state, &self.health);
        let Some(status) = self.status.update(now, derived) else {
            return;
        };
        info!(state = ?status.state, reasons = ?status.reasons, "node status");
        self.ctx
            .live
            .update(self.node, |view| view.status = status.clone());
        self.ctx.publish(Event::Status {
            node: self.node,
            status,
        });
    }
}

/// The next push; pending forever once the link's sender is gone.
async fn recv(
    pushes: &mut Option<broadcast::Receiver<AgentEvent>>,
) -> Result<AgentEvent, broadcast::error::RecvError> {
    match pushes {
        Some(pushes) => pushes.recv().await,
        None => std::future::pending().await,
    }
}
