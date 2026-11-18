//! Weekly node backups (features/lifecycle.md, Backups; D-126): in the nightly window (03:00
//! site time for half an hour, before the RF optimizer's and the rollouts'), Sundays back up every
//! online node, and the other nights the online nodes whose newest backup is over [`LATE_S`]
//! old (offline or busy on Sunday), as one backup job a night. A backup of unchanged files only
//! marks the newest seen (D-126), so most weeks store nothing new. While another job holds one
//! of the nodes, it tries again within the window.

use std::{collections::BTreeMap, sync::Arc, time::Duration};

use tokio::{sync::watch, task::JoinHandle};
use tracing::{info, warn};
use uniwrt_core::{Clock, NodeId, Timestamp};
use uniwrt_engine::Engine;
use uniwrt_jobs::Jobs;
use uniwrt_model::{inventory::BackupReason, site::tz, status::NodeState};
use uniwrt_store::{BackupRepo as _, NodeRepo as _, Store};

use crate::{auto_update::site_offset, fleet::Fleet, fleet_link::FleetLink};

/// How often it looks whether the window is open.
const LOOK_EVERY: Duration = Duration::from_mins(10);
/// The window: 03:00 local, for 30 minutes.
const AT_MIN: u16 = 3 * 60;
const WINDOW_MIN: u16 = 30;
/// A node whose newest backup is older than this is backed up on any night.
const LATE_S: i64 = uniwrt_model::inventory::BACKUP_EVERY_S + 86_400;

/// The task, stopped with [`ScheduledBackups::stop`].
pub(crate) struct ScheduledBackups(JoinHandle<()>, watch::Sender<bool>);

struct Ctx {
    store: Store,
    jobs: Jobs,
    engine: Engine<FleetLink>,
    clock: Arc<dyn Clock>,
}

impl ScheduledBackups {
    pub(crate) fn start(fleet: &Fleet, store: &Store, jobs: &Jobs, clock: &Arc<dyn Clock>) -> Self {
        let ctx = Ctx {
            store: store.clone(),
            jobs: jobs.clone(),
            engine: fleet.engine.clone(),
            clock: Arc::clone(clock),
        };
        let (stop, mut stopped) = watch::channel(false);
        let task = tokio::spawn(async move {
            let mut done = None;
            let mut look = tokio::time::interval(LOOK_EVERY);
            look.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    _ = look.tick() => {}
                    _ = stopped.changed() => return,
                }
                ctx.look(&mut done).await;
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
    /// One look: in the window of a night not yet `done`, starts the backup of the nodes due.
    async fn look(&self, done: &mut Option<i64>) {
        let now = self.clock.now();
        let offset = site_offset(&self.store, now).await;
        let Some(night) = tz::daily_window(now, offset, AT_MIN, WINDOW_MIN) else {
            return;
        };
        if *done == Some(night) {
            return;
        }
        let (nodes, seen) = match (self.store.nodes().await, self.store.backups_seen().await) {
            (Ok(nodes), Ok(seen)) => (nodes, seen),
            (Err(err), _) | (_, Err(err)) => {
                return warn!(%err, "scheduled backups: cannot read the nodes");
            }
        };
        let online: Vec<NodeId> = nodes
            .iter()
            .map(|node| node.id)
            .filter(|&id| {
                self.engine
                    .node(id)
                    .is_some_and(|view| view.status.state != NodeState::Offline)
            })
            .collect();
        let due = due(&online, &seen, now, sunday(night));
        if due.is_empty() {
            *done = Some(night);
            return;
        }
        match self.jobs.back_up(&due, BackupReason::Scheduled, None).await {
            Ok(job) => {
                *done = Some(night);
                info!(job = %job.id, nodes = due.len(), "scheduled backups: started");
            }
            Err(uniwrt_jobs::Error::Busy { target }) => {
                info!(%target, "scheduled backups: waiting for the running job");
            }
            Err(err) => {
                *done = Some(night);
                warn!(%err, "scheduled backups: cannot start");
            }
        }
    }
}

/// Whether local day `day` (days since the epoch) is a Sunday: 1970-01-01 was a Thursday.
fn sunday(day: i64) -> bool {
    (day + 4).rem_euclid(7) == 0
}

/// The `online` nodes to back up tonight: all on a Sunday, else those never backed up or whose
/// newest backup is over [`LATE_S`] old.
fn due(
    online: &[NodeId],
    seen: &BTreeMap<NodeId, Timestamp>,
    now: Timestamp,
    sunday: bool,
) -> Vec<NodeId> {
    online
        .iter()
        .copied()
        .filter(|id| {
            sunday
                || seen
                    .get(id)
                    .is_none_or(|&last| now.as_unix_ms() - last.as_unix_ms() > LATE_S * 1000)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sundays_are_found() {
        // 2026-11-15 was a Sunday.
        let day = Timestamp::parse_utc("2026-11-15T03:00:00Z")
            .unwrap()
            .as_unix_ms()
            .div_euclid(86_400_000);
        assert!(sunday(day));
        assert!(!sunday(day + 1));
        assert!(sunday(day + 7));
    }

    #[test]
    fn sundays_take_every_node_and_other_nights_the_late_ones() {
        let now = Timestamp::from_unix_secs(100 * 86_400);
        let ids = [NodeId::new(1), NodeId::new(2), NodeId::new(3)];
        let seen = BTreeMap::from([
            (ids[0], Timestamp::from_unix_secs(99 * 86_400)),
            (ids[1], Timestamp::from_unix_secs(91 * 86_400)),
        ]);
        assert_eq!(due(&ids, &seen, now, true), ids);
        assert_eq!(due(&ids, &seen, now, false), [ids[1], ids[2]]);
        assert_eq!(due(&ids[..1], &seen, now, false), []);
    }
}
