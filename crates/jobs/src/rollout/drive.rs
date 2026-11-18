//! The driver: one task that moves the active rollout on (D-125). Each turn decides under the
//! edit lock what comes next ([`decide`]) and saves what it found: a node's job ended (upgraded,
//! or failed: halted), the canary's soak, the window, the next node readied. The slow parts run
//! outside the lock ([`act`]) and re-check the record before they save. It looks again when
//! woken (an edit), when an upgrade job ends, or after [`RolloutTiming`]'s `look` at the latest.

use std::{sync::Arc, time::Duration};

use tokio::{sync::broadcast::error::RecvError, time::Instant};
use tracing::{info, warn};
use uniwrt_core::{NodeId, RolloutId, Timestamp};
use uniwrt_model::{
    job::{JobKind, JobState},
    rollout::{Rollout, RolloutNodeState, RolloutState, RolloutWait, UpgradeChoice},
    site::tz,
};
use uniwrt_store::{JobRepo as _, RevisionRepo as _, RolloutRepo as _, Store};

use super::{Inner, canary};
use crate::{Error, UpgradeRequest};

/// What the driver does next, outside the lock.
enum Action {
    Wait(Duration),
    /// Check the canary at the end of its soak.
    Canary {
        id: RolloutId,
        node: NodeId,
        since: Timestamp,
        release: Option<String>,
    },
    /// Ready the node's image, then start its upgrade.
    Start {
        id: RolloutId,
        node: NodeId,
        choice: UpgradeChoice,
    },
}

pub(super) async fn run(inner: Arc<Inner>) {
    let mut jobs = inner.jobs.subscribe();
    loop {
        let wait = act(&inner).await;
        let deadline = Instant::now() + wait;
        loop {
            tokio::select! {
                () = inner.wake.notified() => break,
                () = tokio::time::sleep_until(deadline) => break,
                update = jobs.recv() => match update {
                    Ok(job) if job.kind == JobKind::Upgrade && job.state != JobState::Running => break,
                    Ok(_) => {}
                    Err(RecvError::Lagged(_)) => break,
                    Err(RecvError::Closed) => return,
                },
            }
        }
    }
}

/// One turn; how long until the next at the latest.
async fn act(inner: &Inner) -> Duration {
    let action = match decide(inner).await {
        Ok(action) => action,
        Err(err) => {
            warn!(%err, "rollout: cannot read or save the rollout");
            return inner.timing.look;
        }
    };
    let outcome = match action {
        Action::Wait(wait) => return wait,
        Action::Canary {
            id,
            node,
            since,
            release,
        } => {
            let found = canary::check(&inner.jobs.shared, node, since, release.as_deref()).await;
            settle_canary(inner, id, node, found).await
        }
        Action::Start { id, node, choice } => {
            let images = inner.images.get().cloned();
            let request = match images {
                Some(images) => images.request(node, &choice).await,
                None => Err("The controller can't read images".to_owned()),
            };
            start(inner, id, node, request).await
        }
    };
    outcome.unwrap_or_else(|err| {
        warn!(%err, "rollout: cannot save the rollout");
        inner.timing.look
    })
}

async fn decide(inner: &Inner) -> crate::Result<Action> {
    let _edit = inner.edit.lock().await;
    let store = &inner.jobs.shared.store;
    let look = inner.timing.look;
    let Some(mut rollout) = store.active_rollout().await? else {
        return Ok(Action::Wait(look));
    };
    let before = rollout.clone();
    let now = inner.jobs.shared.clock.now();
    for node in &mut rollout.nodes {
        // Left preparing by a stop: nothing started on it.
        if node.state == RolloutNodeState::Preparing {
            node.state = RolloutNodeState::Pending;
            node.detail = None;
        }
    }
    let upgrading = rollout
        .nodes
        .iter()
        .position(|node| node.state == RolloutNodeState::Upgrading);
    if let Some(index) = upgrading {
        let job = match rollout.nodes[index].job {
            Some(id) => store.job(id).await?,
            None => None,
        };
        if job
            .as_ref()
            .is_some_and(|job| job.state == JobState::Running)
        {
            save_if_changed(inner, &before, &rollout).await?;
            return Ok(Action::Wait(look));
        }
        ended(&mut rollout, index, job, now);
    }
    let action = next(inner, store, &mut rollout, now).await?;
    save_if_changed(inner, &before, &rollout).await?;
    Ok(action)
}

/// Records how the upgrading node's job ended: upgraded (the first one soaks as the canary), or
/// failed and the rollout halted.
fn ended(rollout: &mut Rollout, index: usize, job: Option<uniwrt_model::job::Job>, now: Timestamp) {
    let succeeded = job
        .as_ref()
        .is_some_and(|job| job.state == JobState::Succeeded);
    let node = &mut rollout.nodes[index];
    if !succeeded {
        let why = job.and_then(|job| job.error).map_or_else(
            || "Its upgrade job is gone".to_owned(),
            |error| error.message,
        );
        node.state = RolloutNodeState::Failed;
        node.detail = Some(why.clone());
        rollout.error = Some(format!("{}'s upgrade failed: {why}", node.name));
        rollout.state = RolloutState::Halted;
        rollout.waiting = None;
        return;
    }
    node.state = RolloutNodeState::Upgraded;
    node.detail = Some(match node.choice.release() {
        Some(release) => format!("Upgraded to {release}"),
        None => "Upgraded".to_owned(),
    });
    let upgraded = rollout
        .nodes
        .iter()
        .filter(|node| node.state == RolloutNodeState::Upgraded)
        .count();
    let more = rollout
        .nodes
        .iter()
        .any(|node| node.state == RolloutNodeState::Pending);
    if let (Some(canary), 1, true) = (rollout.canary, upgraded, more) {
        let soak = Duration::from_secs(u64::from(canary.soak_min) * 60);
        rollout.waiting = Some(RolloutWait::Canary {
            until: now.saturating_add(soak),
        });
        if let Some(detail) = &mut rollout.nodes[index].detail {
            detail.push_str("; soaking as the canary");
        }
    }
}

/// What a running rollout does next: wait (the canary, the window), check the canary, start the
/// next node, or end.
async fn next(
    inner: &Inner,
    store: &Store,
    rollout: &mut Rollout,
    now: Timestamp,
) -> crate::Result<Action> {
    let look = inner.timing.look;
    if rollout.state != RolloutState::Running {
        return Ok(Action::Wait(look));
    }
    match rollout.waiting.clone() {
        Some(RolloutWait::Canary { until }) => {
            if now < until {
                return Ok(Action::Wait(until.saturating_duration_since(now).min(look)));
            }
            let soak =
                Duration::from_secs(u64::from(rollout.canary.map_or(0, |c| c.soak_min)) * 60);
            let canary = rollout
                .nodes
                .iter()
                .find(|node| node.state == RolloutNodeState::Upgraded);
            if let Some(canary) = canary {
                return Ok(Action::Canary {
                    id: rollout.id,
                    node: canary.node,
                    since: until.saturating_sub(soak),
                    release: canary.choice.release().map(str::to_owned),
                });
            }
            rollout.waiting = None;
        }
        // Looked at again below.
        Some(RolloutWait::Window { .. } | RolloutWait::Busy { .. }) => rollout.waiting = None,
        None => {}
    }
    let Some(index) = rollout
        .nodes
        .iter()
        .position(|node| node.state == RolloutNodeState::Pending)
    else {
        rollout.state = RolloutState::Done;
        rollout.finished_at = Some(now);
        info!(rollout = %rollout.id, "rollout done");
        return Ok(Action::Wait(look));
    };
    if let Some(window) = rollout.window {
        let offset = site_offset(store, now).await;
        if !window.is_open(now, offset) {
            let opens = window.opens(now, offset);
            rollout.waiting = Some(RolloutWait::Window { opens });
            return Ok(Action::Wait(opens.saturating_duration_since(now).min(look)));
        }
    }
    let node = &mut rollout.nodes[index];
    node.state = RolloutNodeState::Preparing;
    node.detail = Some(match node.choice {
        UpgradeChoice::Asu { .. } => "Waiting for ASU's image".to_owned(),
        _ => "Planning its upgrade".to_owned(),
    });
    Ok(Action::Start {
        id: rollout.id,
        node: node.node,
        choice: node.choice.clone(),
    })
}

/// Records the canary's check: on, or halted with why (the canary stays upgraded: resuming goes
/// on with the rest).
async fn settle_canary(
    inner: &Inner,
    id: RolloutId,
    node: NodeId,
    found: Result<String, String>,
) -> crate::Result<Duration> {
    let _edit = inner.edit.lock().await;
    let store = &inner.jobs.shared.store;
    let Some(mut rollout) = store.rollout(id).await? else {
        return Ok(Duration::ZERO);
    };
    if !rollout.state.active() || !matches!(rollout.waiting, Some(RolloutWait::Canary { .. })) {
        return Ok(Duration::ZERO);
    }
    let before = rollout.clone();
    rollout.waiting = None;
    let Some(canary) = rollout.nodes.iter_mut().find(|entry| entry.node == node) else {
        return Ok(Duration::ZERO);
    };
    match found {
        Ok(found) => canary.detail = Some(found),
        Err(why) => {
            canary.detail = Some(why.clone());
            rollout.error = Some(format!(
                "The canary {} failed its check: {why}. Resume to go on with the rest anyway, \
                 or cancel",
                canary.name
            ));
            rollout.state = RolloutState::Halted;
        }
    }
    save_if_changed(inner, &before, &rollout).await?;
    Ok(Duration::ZERO)
}

/// Starts the readied node's upgrade, unless the rollout changed meanwhile.
async fn start(
    inner: &Inner,
    id: RolloutId,
    node: NodeId,
    request: Result<UpgradeRequest, String>,
) -> crate::Result<Duration> {
    let _edit = inner.edit.lock().await;
    let store = &inner.jobs.shared.store;
    let Some(mut rollout) = store.rollout(id).await? else {
        return Ok(Duration::ZERO);
    };
    let before = rollout.clone();
    let by = rollout.created_by;
    let running = rollout.state == RolloutState::Running;
    let Some(index) = rollout.nodes.iter().position(|entry| entry.node == node) else {
        return Ok(Duration::ZERO);
    };
    if rollout.nodes[index].state != RolloutNodeState::Preparing {
        return Ok(Duration::ZERO);
    }
    let name = rollout.nodes[index].name.clone();
    let mut wait = Duration::ZERO;
    let entry = &mut rollout.nodes[index];
    let outcome = if running {
        match request {
            Err(why) => Err(why),
            Ok(request) => match inner.jobs.upgrade(request, by).await {
                Ok(job) => Ok(Some(job.id)),
                Err(Error::Busy { .. }) => Ok(None),
                Err(Error::Observed(_)) => {
                    Err("It is in observe mode: turn its management on, then resume".to_owned())
                }
                Err(err) => Err(err.to_string()),
            },
        }
    } else {
        entry.state = RolloutNodeState::Pending;
        entry.detail = Some("Paused before it started".to_owned());
        Ok(None)
    };
    match outcome {
        Ok(Some(job)) => {
            entry.state = RolloutNodeState::Upgrading;
            entry.job = Some(job);
            entry.detail = Some(match entry.choice.release() {
                Some(release) => format!("Upgrading to {release}"),
                None => "Upgrading".to_owned(),
            });
        }
        Ok(None) if running => {
            entry.state = RolloutNodeState::Pending;
            entry.detail = None;
            rollout.waiting = Some(RolloutWait::Busy {
                why: format!("Another job runs on {name}: trying again shortly"),
            });
            wait = inner.timing.retry;
        }
        Ok(None) => {}
        Err(why) => {
            entry.state = RolloutNodeState::Failed;
            entry.detail = Some(why.clone());
            rollout.error = Some(format!("{name} couldn't start its upgrade: {why}"));
            rollout.state = RolloutState::Halted;
        }
    }
    save_if_changed(inner, &before, &rollout).await?;
    Ok(wait)
}

async fn save_if_changed(inner: &Inner, before: &Rollout, rollout: &Rollout) -> crate::Result<()> {
    if before != rollout {
        inner.jobs.shared.store.update_rollout(rollout).await?;
        let _ = inner.updates.send(Arc::new(rollout.clone()));
    }
    Ok(())
}

/// The site's offset from UTC at `now`, in seconds east (UTC without a site).
async fn site_offset(store: &Store, now: Timestamp) -> i32 {
    match store.head_revision().await {
        Ok(Some(head)) => tz::posix(&head.config.site.timezone)
            .and_then(|posix| tz::local(posix, now))
            .map_or(0, |local| local.offset_s),
        Ok(None) => 0,
        Err(err) => {
            warn!(%err, "rollout: cannot read the site's time zone");
            0
        }
    }
}
