//! Scheduled restarts (features/network.md, Other services; D-138): every minute, the managed
//! nodes whose restart time (the site's, or their own) came within the last
//! [`REBOOT_WINDOW_MIN`] are restarted as one reboot job (D-137's: farthest first, the gateway
//! last), unless a scheduled restart still runs. A node offline or held by a job waits within its
//! window; one the window closes on unrestarted raises `scheduled_reboot_missed`, as does each
//! node a failed job left as it was. Each node is tried once per window: what was tried is kept
//! in memory, and after a controller restart an uptime shorter than the time since the window
//! opened counts as done, so nothing restarts twice. Nothing runs while the site's time zone
//! can't be read, or while the controller is down.

use std::{collections::BTreeMap, sync::Arc, time::Duration};

use tokio::{sync::watch, task::JoinHandle};
use tracing::{info, warn};
use uniwrt_core::{Clock, JobId, NodeId, Timestamp};
use uniwrt_engine::Engine;
use uniwrt_jobs::Jobs;
use uniwrt_model::{
    events::{EventKind, NewEvent, RebootMissed},
    job::{JobNodeState, JobState},
    node::NodeMode,
    site::{
        schedules::{REBOOT_WINDOW_MIN, RebootTime},
        tz,
    },
    status::NodeState,
};
use uniwrt_store::{NodeRepo as _, RevisionRepo as _, Store};

use crate::{fleet::Fleet, fleet_link::FleetLink};

/// How often it looks whether a node is due.
const LOOK_EVERY: Duration = Duration::from_mins(1);
/// A node tried this recently isn't tried again: longer than a window, so a daylight saving
/// change that repeats the hour doesn't restart it twice, and shorter than a day.
const TRIED_FOR_S: i64 = 20 * 3600;

/// The task, stopped with [`ScheduledReboots::stop`].
pub(crate) struct ScheduledReboots(JoinHandle<()>, watch::Sender<bool>);

struct Ctx {
    store: Store,
    jobs: Jobs,
    engine: Engine<FleetLink>,
    clock: Arc<dyn Clock>,
}

/// What the task remembers between looks.
#[derive(Debug, Default)]
struct Memory {
    /// When each node was last put in a job, or found restarted.
    tried: BTreeMap<NodeId, Timestamp>,
    /// The nodes due that couldn't go yet, and why.
    waiting: BTreeMap<NodeId, RebootMissed>,
    /// The scheduled restart running now.
    running: Option<JobId>,
}

/// One managed node with a restart time, as a look sees it.
#[derive(Debug, Clone)]
struct Seen {
    id: NodeId,
    time: RebootTime,
    online: bool,
    busy: bool,
    uptime_s: Option<u64>,
}

/// What a look found.
#[derive(Debug, Default, PartialEq, Eq)]
struct Look {
    /// To restart now.
    due: Vec<NodeId>,
    /// Restarted since their window opened.
    done: Vec<NodeId>,
    /// Due, but not free yet.
    waiting: Vec<(NodeId, RebootMissed)>,
    /// Whose window closed while they waited.
    missed: Vec<(NodeId, RebootMissed)>,
}

impl ScheduledReboots {
    pub(crate) fn start(fleet: &Fleet, store: &Store, jobs: &Jobs, clock: &Arc<dyn Clock>) -> Self {
        let ctx = Ctx {
            store: store.clone(),
            jobs: jobs.clone(),
            engine: fleet.engine.clone(),
            clock: Arc::clone(clock),
        };
        let (stop, mut stopped) = watch::channel(false);
        let task = tokio::spawn(async move {
            let mut memory = Memory::default();
            let mut look = tokio::time::interval(LOOK_EVERY);
            look.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    _ = look.tick() => {}
                    _ = stopped.changed() => return,
                }
                ctx.look(&mut memory).await;
            }
        });
        Self(task, stop)
    }

    pub(crate) async fn stop(self) {
        let _ = self.1.send(true);
        let _ = self.0.await;
    }
}

impl Ctx {
    async fn look(&self, memory: &mut Memory) {
        let now = self.clock.now();
        self.follow(memory, now).await;
        let config = match self.store.head_revision().await {
            Ok(Some(head)) => head.config,
            Ok(None) => return,
            Err(err) => return warn!(%err, "scheduled restarts: cannot read the site"),
        };
        let Some(local) = tz::posix(&config.site.timezone).and_then(|p| tz::local(p, now)) else {
            return;
        };
        let nodes = match self.store.nodes().await {
            Ok(nodes) => nodes,
            Err(err) => return warn!(%err, "scheduled restarts: cannot read the nodes"),
        };
        let seen: Vec<Seen> = nodes
            .iter()
            .filter(|node| node.mode == NodeMode::Managed)
            .filter_map(|node| {
                let time = config.reboot_time(node.id)?.clone();
                let view = self.engine.node(node.id);
                Some(Seen {
                    id: node.id,
                    time,
                    online: view
                        .as_ref()
                        .is_some_and(|v| v.status.state != NodeState::Offline),
                    busy: self.jobs.holds(node.id),
                    uptime_s: view
                        .as_ref()
                        .and_then(|v| v.facts.as_ref())
                        .map(|f| f.system.uptime_s),
                })
            })
            .collect();
        let found = decide(&seen, local, now, memory);
        // A node no longer scheduled (or no longer managed) waits for nothing.
        memory
            .waiting
            .retain(|id, _| seen.iter().any(|node| node.id == *id));
        for id in found.done {
            memory.tried.insert(id, now);
            memory.waiting.remove(&id);
        }
        for (id, why) in found.missed {
            memory.waiting.remove(&id);
            self.missed(id, why, now);
        }
        memory.waiting.extend(found.waiting);
        if found.due.is_empty() {
            return;
        }
        if memory.running.is_some() {
            // One restart at a time: these wait for the one running.
            for &id in &found.due {
                memory.waiting.insert(id, RebootMissed::Busy);
            }
            return;
        }
        match self.jobs.reboot(&found.due, None).await {
            Ok(job) => {
                info!(job = %job.id, nodes = found.due.len(), "scheduled restarts: started");
                memory.running = Some(job.id);
                for id in found.due {
                    memory.tried.insert(id, now);
                    memory.waiting.remove(&id);
                }
            }
            Err(uniwrt_jobs::Error::Busy { target }) => {
                info!(%target, "scheduled restarts: waiting for the running job");
                for id in found.due {
                    memory.waiting.insert(id, RebootMissed::Busy);
                }
            }
            Err(err) => {
                warn!(%err, "scheduled restarts: cannot start");
                for id in found.due {
                    memory.tried.insert(id, now);
                    memory.waiting.remove(&id);
                    self.missed(id, RebootMissed::JobFailed, now);
                }
            }
        }
    }

    /// Once the running restart ends, notes the nodes it left as they were.
    async fn follow(&self, memory: &mut Memory, now: Timestamp) {
        let Some(id) = memory.running else { return };
        let job = match self.jobs.job(id).await {
            Ok(Some(job)) => job,
            Ok(None) => {
                memory.running = None;
                return;
            }
            Err(err) => return warn!(%err, "scheduled restarts: cannot read the job"),
        };
        if job.state == JobState::Running {
            return;
        }
        memory.running = None;
        for node in job
            .nodes
            .iter()
            .filter(|n| n.state != JobNodeState::Verified)
        {
            self.missed(node.node, RebootMissed::JobFailed, now);
        }
    }

    fn missed(&self, node: NodeId, why: RebootMissed, now: Timestamp) {
        warn!(%node, ?why, "scheduled restarts: missed");
        let what = EventKind::ScheduledRebootMissed { why };
        self.engine.note(NewEvent::new(now, what).node(node));
    }
}

/// Which of `nodes` to restart at `local` time (`now`), which wait, and which missed their
/// window, given what `memory` holds.
fn decide(nodes: &[Seen], local: tz::Local, now: Timestamp, memory: &Memory) -> Look {
    let mut look = Look::default();
    let now_s = now.as_unix_ms().div_euclid(1000);
    for node in nodes {
        let Some(since) = node.time.since(local.day, local.minute) else {
            continue;
        };
        if since >= REBOOT_WINDOW_MIN {
            if let Some(&why) = memory.waiting.get(&node.id) {
                look.missed.push((node.id, why));
            }
            continue;
        }
        let tried = memory
            .tried
            .get(&node.id)
            .is_some_and(|at| now_s - at.as_unix_ms().div_euclid(1000) < TRIED_FOR_S);
        if tried {
            continue;
        }
        // Seconds since the window opened: whole minutes of local time, plus the seconds of
        // this one (zones are whole minutes off UTC).
        let opened_s = u64::from(since) * 60 + now_s.rem_euclid(60).unsigned_abs();
        if node.uptime_s.is_some_and(|up| up < opened_s) {
            look.done.push(node.id);
        } else if !node.online {
            look.waiting.push((node.id, RebootMissed::Offline));
        } else if node.busy {
            look.waiting.push((node.id, RebootMissed::Busy));
        } else {
            look.due.push(node.id);
        }
    }
    look
}

#[cfg(test)]
mod tests {
    use uniwrt_model::site::wireless::Weekday;

    use super::*;

    const SUNDAY_4AM: u16 = 4 * 60;
    /// A minute's start, so the seconds since the window opened are whole minutes.
    const NOW: Timestamp = Timestamp::from_unix_secs(1_800_000_000);

    fn local(day: Weekday, minute: u16) -> tz::Local {
        tz::Local {
            day,
            minute,
            offset_s: 0,
        }
    }

    fn seen(id: i64) -> Seen {
        Seen {
            id: NodeId::new(id),
            time: RebootTime {
                days: vec![Weekday::Sun],
                at: SUNDAY_4AM,
            },
            online: true,
            busy: false,
            uptime_s: Some(86_400),
        }
    }

    #[test]
    fn nodes_are_due_within_their_window_once() {
        let nodes = [seen(1), seen(2)];
        let mut memory = Memory::default();
        let before = decide(&nodes, local(Weekday::Sun, SUNDAY_4AM - 1), NOW, &memory);
        assert_eq!(before, Look::default());
        let at = decide(&nodes, local(Weekday::Sun, SUNDAY_4AM + 10), NOW, &memory);
        assert_eq!(at.due, [NodeId::new(1), NodeId::new(2)]);

        memory.tried.insert(NodeId::new(1), NOW);
        let later = Timestamp::from_unix_secs(NOW.as_unix_ms() / 1000 + 600);
        let again = decide(&nodes, local(Weekday::Sun, SUNDAY_4AM + 20), later, &memory);
        assert_eq!(again.due, [NodeId::new(2)]);
        // A week on, it is due again.
        let week = Timestamp::from_unix_secs(NOW.as_unix_ms() / 1000 + 7 * 86_400);
        let next = decide(&nodes, local(Weekday::Sun, SUNDAY_4AM), week, &memory);
        assert_eq!(next.due, [NodeId::new(1), NodeId::new(2)]);
    }

    #[test]
    fn a_node_up_for_less_than_the_window_restarted_already() {
        // 10 minutes into the window: one up for 5 minutes restarted in it, one up for 11
        // didn't.
        let nodes = [
            Seen {
                uptime_s: Some(300),
                ..seen(1)
            },
            Seen {
                uptime_s: Some(660),
                ..seen(2)
            },
        ];
        let look = decide(
            &nodes,
            local(Weekday::Sun, SUNDAY_4AM + 10),
            NOW,
            &Memory::default(),
        );
        assert_eq!(look.done, [NodeId::new(1)]);
        assert_eq!(look.due, [NodeId::new(2)]);
    }

    #[test]
    fn offline_and_busy_nodes_wait_and_miss_when_the_window_closes() {
        let nodes = [
            Seen {
                online: false,
                ..seen(1)
            },
            Seen {
                busy: true,
                ..seen(2)
            },
            seen(3),
        ];
        let mut memory = Memory::default();
        let look = decide(&nodes, local(Weekday::Sun, SUNDAY_4AM), NOW, &memory);
        assert_eq!(
            look.waiting,
            [
                (NodeId::new(1), RebootMissed::Offline),
                (NodeId::new(2), RebootMissed::Busy)
            ]
        );
        assert_eq!(look.due, [NodeId::new(3)]);
        memory.waiting.extend(look.waiting);
        memory.tried.insert(NodeId::new(3), NOW);

        let closed = decide(&nodes, local(Weekday::Sun, SUNDAY_4AM + 60), NOW, &memory);
        assert_eq!(
            closed.missed,
            [
                (NodeId::new(1), RebootMissed::Offline),
                (NodeId::new(2), RebootMissed::Busy)
            ]
        );
        assert!(closed.due.is_empty() && closed.waiting.is_empty());
    }
}
