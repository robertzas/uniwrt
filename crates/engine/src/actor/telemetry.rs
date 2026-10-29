//! What an actor does with telemetry: collected sets become the node's facts, pushed events are
//! stamped and announced (and refresh the sets they touch), log batches are announced without
//! the entries a restarted follow repeats.

use std::sync::Arc;

use tokio::{sync::broadcast::error::RecvError, time::Instant};
use tracing::warn;
use uniwrt_core::Timestamp;
use uniwrt_model::{event::NodeEvent, facts::NodeFacts, log::LogEntry};
use uniwrt_node::{
    AgentEvent, NodeLink,
    events::{self, Push},
    facts,
    proto::ops::{Collected, Set},
};

use super::{LOG_RESTART, NodeActor};
use crate::bus::Event;

/// The sets that pushed events keep fresh, collected again when pushes were missed.
const EVENT_SETS: [Set; 4] = [Set::Devices, Set::Interfaces, Set::Leases, Set::Stations];

impl<L: NodeLink> NodeActor<L> {
    pub(super) fn on_push(&mut self, push: Result<AgentEvent, RecvError>) {
        match push {
            Ok(event) => self.on_agent_event(&event),
            Err(RecvError::Lagged(skipped)) => {
                warn!(skipped, "missed pushes of the agent; collecting again");
                for set in EVENT_SETS {
                    self.mark_dirty(set);
                }
            }
            Err(RecvError::Closed) => self.pushes = None,
        }
    }

    fn on_agent_event(&mut self, event: &AgentEvent) {
        let reported_at = match event {
            AgentEvent::Event { ts, .. } => *ts,
            AgentEvent::Tick(_) => None,
        };
        match events::parse(event) {
            Ok(Some(Push::Tick(collected))) => self.apply(&collected),
            Ok(Some(Push::Event(event))) => self.on_node_event(reported_at, event),
            Ok(Some(Push::Log { entries, dropped })) => self.on_log(entries, dropped),
            Ok(Some(Push::LogEnded { reason })) => {
                warn!(%reason, "the node stopped following its log");
                self.logs.following = false;
                self.logs.retry_at = Some(Instant::now() + LOG_RESTART);
            }
            Ok(None) => {}
            Err(err) => warn!(%err, "a push of the wrong shape"),
        }
    }

    /// Applies collected sets to the node's facts, and announces them if they changed.
    pub(super) fn apply(&mut self, collected: &Collected) {
        let now = self.ctx.clock.now();
        let before = self.facts.clone();
        let mut next = before.as_deref().cloned().unwrap_or_default();
        let errors = facts::apply(collected, &mut next);
        next.collected_at = now;
        for set in collected.sets.keys() {
            self.health.sets.remove(set);
        }
        for error in errors {
            self.health.sets.insert(error.set, error.reason);
        }
        self.health.stale = false;
        self.health.last_update = Some(now);
        self.stale_from = Instant::now();
        if collected.sets.contains_key(&Set::Stations)
            && !self.health.sets.contains_key(&Set::Stations)
        {
            self.ctx.roams().seed(self.node, now, &next.stations);
        }
        let changed: Vec<_> = match &before {
            Some(before) => Set::ALL
                .into_iter()
                .filter(|&set| differs(set, before, &next))
                .collect(),
            None => collected
                .sets
                .keys()
                .copied()
                .filter(|set| !self.health.sets.contains_key(set))
                .collect(),
        };
        if before.is_some() && changed.is_empty() {
            return;
        }
        let facts = Arc::new(next);
        self.facts = Some(Arc::clone(&facts));
        self.ctx
            .live
            .update(self.node, |view| view.facts = Some(Arc::clone(&facts)));
        self.ctx.publish(Event::Facts {
            node: self.node,
            facts,
            changed,
        });
    }

    fn on_node_event(&mut self, reported_at: Option<Timestamp>, event: NodeEvent) {
        let at = self.ctx.clock.now();
        let roam = match &event {
            NodeEvent::Station(station) => self.ctx.roams().observe(self.node, at, station),
            _ => None,
        };
        self.mark_dirty(match &event {
            NodeEvent::Station(_) => Set::Stations,
            NodeEvent::Device(_) => Set::Devices,
            NodeEvent::Interface(_) => Set::Interfaces,
            NodeEvent::Lease(_) => Set::Leases,
        });
        self.ctx.publish(Event::Node {
            node: self.node,
            at,
            reported_at,
            event,
        });
        if let Some(roam) = roam {
            self.ctx.publish(Event::Roam { at, roam });
        }
    }

    /// Collects `set` again shortly, with whatever else events touch meanwhile.
    fn mark_dirty(&mut self, set: Set) {
        if !self
            .session
            .as_ref()
            .is_some_and(|session| session.caps.has_set(set))
        {
            return;
        }
        self.refresh.insert(set);
        let delay = self.ctx.config.refresh_delay;
        self.refresh_at
            .get_or_insert_with(|| Instant::now() + delay);
    }

    fn on_log(&mut self, mut entries: Vec<LogEntry>, dropped: u64) {
        if let Some(last) = self.logs.resume.take()
            && let Some(repeated) = entries.iter().rposition(|entry| *entry == last)
        {
            entries.drain(..=repeated);
        }
        if let Some(last) = entries.last() {
            self.logs.last = Some(last.clone());
        }
        if entries.is_empty() && dropped == 0 {
            return;
        }
        self.ctx.publish(Event::Log {
            node: self.node,
            entries: entries.into(),
            dropped,
        });
    }
}

/// The part of the facts that `set` covers differs.
fn differs(set: Set, a: &NodeFacts, b: &NodeFacts) -> bool {
    match set {
        Set::Batman => a.batman != b.batman,
        Set::Board => a.board != b.board,
        Set::Capabilities => a.capabilities != b.capabilities,
        Set::Devices => a.devices != b.devices,
        Set::Interfaces => a.interfaces != b.interfaces,
        Set::Leases => a.leases != b.leases,
        Set::Neighbors => a.neighbors != b.neighbors || a.fdb != b.fdb,
        Set::Phys => a.phys != b.phys,
        Set::Radios => a.radios != b.radios,
        Set::Stations => a.stations != b.stations,
        Set::System => a.system != b.system,
    }
}
