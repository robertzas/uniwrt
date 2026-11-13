//! The nightly RF optimizer (features/wifi.md, RF planner: Apply; M6.1 b3a, D-115): with
//! `nightly` on in the `rf` settings, the site's window (03:30 local by default, for two hours)
//! plans every band and applies the bands whose cost improves by 15 % or more
//! ([`uniwrt_rf::plan::nightly`]): the plan is saved as a revision by the controller and applied
//! with the usual transactions and rollback. It never applies what nobody reviewed (unapplied
//! changes skip the night), and it waits while a radio that would move carries heavy traffic,
//! a managed node is offline or another job holds the configuration; a window that closes
//! while it waits is noted as skipped. Then, and every night the optimizer is off, the window
//! restarts the radios still catching up after a channel switch ([`catch_up`], M6.1 b3b,
//! D-116).
//!
//! The decisions run over [`Site`], what the controller holds ([`ctx::Ctx`]), so tests script it.

mod catch_up;
mod ctx;
#[cfg(test)]
mod tests;

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};

use tokio::{sync::watch, task::JoinHandle};
use tracing::{info, warn};
use uniwrt_api::Api;
use uniwrt_core::{Clock, JobId, NodeId, RevisionId, Timestamp};
use uniwrt_jobs::Jobs;
use uniwrt_model::{
    events::EventKind,
    node::NodeMode,
    rf::{BandGain, NightlySkip, RfSettings, nightly_window},
    site::{SiteConfig, tz},
    wifi::Band,
};
use uniwrt_rf::plan::{
    Plan,
    nightly::{self, Nightly},
};
use uniwrt_store::Store;

use crate::fleet::Fleet;

/// How often it looks whether the window is open.
const LOOK_EVERY: Duration = Duration::from_mins(10);

/// The optimizer's task; nothing runs without RF insight (no fleet).
pub(crate) struct NightlyOptimizer(Option<(JoinHandle<()>, watch::Sender<bool>)>);

impl NightlyOptimizer {
    pub(crate) fn start(
        fleet: &Fleet,
        store: &Store,
        jobs: &Jobs,
        api: &Api,
        clock: &Arc<dyn Clock>,
    ) -> Self {
        let Some(rf) = fleet.rf() else {
            return Self(None);
        };
        let site = ctx::Ctx {
            store: store.clone(),
            jobs: jobs.clone(),
            engine: fleet.engine.clone(),
            rf,
            api: api.clone(),
            clock: Arc::clone(clock),
        };
        let (stop, mut stopped) = watch::channel(false);
        let task = tokio::spawn(async move {
            let mut state = State::default();
            let mut look = tokio::time::interval(LOOK_EVERY);
            look.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    _ = look.tick() => {}
                    _ = stopped.changed() => return,
                }
                self::look(&site, &mut state).await;
            }
        });
        Self(Some((task, stop)))
    }

    pub(crate) async fn stop(self) {
        if let Some((task, stop)) = self.0 {
            let _ = stop.send(true);
            let _ = task.await;
        }
    }
}

/// The site's latest revision.
struct Head {
    id: RevisionId,
    config: SiteConfig,
}

/// Why a save didn't happen.
enum SaveError {
    /// Someone saved meanwhile.
    NotLatest,
    Failed(String),
}

/// Why a job (an apply, a radio restart) didn't start.
enum StartError {
    /// Another job holds the configuration or a node.
    Busy,
    Failed(String),
}

/// What the optimizer reads and does; errors are for the log.
trait Site: Send + Sync {
    fn settings(&self) -> RfSettings;
    fn now(&self) -> Timestamp;
    async fn head(&self) -> Result<Option<Head>, String>;
    /// Each node's mode, and the revision each holds.
    async fn nodes(
        &self,
    ) -> Result<(BTreeMap<NodeId, NodeMode>, BTreeMap<NodeId, RevisionId>), String>;
    /// The plan of the site as it runs.
    async fn plan(
        &self,
        config: SiteConfig,
        modes: BTreeMap<NodeId, NodeMode>,
    ) -> Result<Plan, String>;
    /// The radios carrying heavy traffic, by node and name.
    async fn busy(&self) -> Result<BTreeSet<(NodeId, String)>, String>;
    fn offline(&self, node: NodeId) -> bool;
    /// The radios of `node` still catching up after a channel switch, by name.
    fn catching_up(&self, node: NodeId) -> Vec<String>;
    async fn job_running(&self) -> bool;
    /// Validates `config` and saves it after `base` as the controller's revision.
    async fn save(
        &self,
        base: RevisionId,
        config: SiteConfig,
        message: String,
    ) -> Result<RevisionId, SaveError>;
    async fn apply(&self, revision: RevisionId) -> Result<JobId, StartError>;
    /// Starts restarting the radios catching up on `nodes`, one node at a time.
    async fn restart(&self, nodes: Vec<NodeId>) -> Result<JobId, StartError>;
    fn note(&self, kind: EventKind);
}

#[derive(Debug, Default)]
struct State {
    /// The night already decided, as [`nightly_window`] numbers it.
    done: Option<i64>,
    /// Tonight's plan waiting, why, and its bands: noted as skipped if the window closes first.
    waiting: Option<(i64, NightlySkip, Vec<BandGain>)>,
    /// The revision it saved whose apply couldn't start yet: not "unapplied changes" to it.
    /// Forgotten once its apply starts, so an apply that fails (rolled back) is never tried
    /// again by itself: later nights skip until someone applies or discards it.
    ours: Option<RevisionId>,
    /// The nodes whose radios a restart job took this night (or failed to): once a night each.
    restarted: Option<(i64, BTreeSet<NodeId>)>,
}

/// How an attempt ended.
enum Attempt {
    Done,
    Wait(NightlySkip, Vec<BandGain>),
}

async fn look(site: &impl Site, state: &mut State) {
    let settings = site.settings();
    if !settings.nightly {
        state.waiting = None;
    }
    let head = match site.head().await {
        Ok(Some(head)) => head,
        Ok(None) => return,
        Err(err) => return warn!(%err, "nightly RF: cannot read the site"),
    };
    let now = site.now();
    let offset = tz::posix(&head.config.site.timezone)
        .and_then(|posix| tz::local(posix, now))
        .map_or(0, |local| local.offset_s);
    let window = nightly_window(now, offset, settings.nightly_at_min);
    if let Some((_, reason, bands)) = state.waiting.take_if(|(n, ..)| window != Some(*n)) {
        info!(
            ?reason,
            "nightly RF: the window closed while waiting; skipped"
        );
        site.note(EventKind::RfOptimizeSkipped { reason, bands });
    }
    let Some(night) = window else { return };
    if settings.nightly && state.done != Some(night) {
        match attempt(site, head, state).await {
            Attempt::Done => {
                state.done = Some(night);
                state.waiting = None;
            }
            Attempt::Wait(reason, bands) => {
                state.waiting = Some((night, reason, bands));
                return;
            }
        }
    }
    // After tonight's apply, which may have switched radios itself (it holds a job meanwhile).
    catch_up::catch_up(site, state, night).await;
}

async fn attempt(site: &impl Site, head: Head, state: &mut State) -> Attempt {
    let (modes, holds) = match site.nodes().await {
        Ok(nodes) => nodes,
        Err(err) => {
            warn!(%err, "nightly RF: cannot read the nodes");
            return Attempt::Done;
        }
    };
    let managed: Vec<NodeId> = modes
        .iter()
        .filter(|(_, mode)| **mode == NodeMode::Managed)
        .map(|(node, _)| *node)
        .collect();
    let pending =
        state.ours != Some(head.id) && managed.iter().any(|node| holds.get(node) != Some(&head.id));
    let planned = match site.plan(head.config.clone(), modes).await {
        Ok(plan) => site.busy().await.map(|busy| (plan, busy)),
        Err(err) => Err(err),
    };
    let (plan, busy) = match planned {
        Ok(planned) => planned,
        Err(err) => {
            warn!(%err, "nightly RF: cannot plan");
            return Attempt::Done;
        }
    };
    let bands = match nightly::decide(&plan, pending, &busy) {
        Nightly::Nothing => {
            info!("nightly RF: no band improves enough to move radios");
            return Attempt::Done;
        }
        Nightly::PendingChanges(bands) => {
            info!("nightly RF: the site has unapplied changes; skipped");
            site.note(EventKind::RfOptimizeSkipped {
                reason: NightlySkip::PendingChanges,
                bands,
            });
            return Attempt::Done;
        }
        Nightly::Traffic(bands, radios) => {
            info!(?radios, "nightly RF: heavy traffic; trying again later");
            return Attempt::Wait(NightlySkip::Traffic, bands);
        }
        Nightly::Apply(bands) => bands,
    };
    if managed.iter().any(|node| site.offline(*node)) {
        info!("nightly RF: a managed node is offline; trying again later");
        return Attempt::Wait(NightlySkip::Offline, bands);
    }
    if site.job_running().await {
        return Attempt::Wait(NightlySkip::Busy, bands);
    }
    apply(site, head, &plan, bands, state).await
}

/// Saves the worthy bands' plan as the next revision (unless it is the one it saved before)
/// and starts applying it.
async fn apply(
    site: &impl Site,
    head: Head,
    plan: &Plan,
    bands: Vec<BandGain>,
    state: &mut State,
) -> Attempt {
    let mut config = head.config.clone();
    for band in plan.bands.iter().filter(|band| nightly::worth(band)) {
        nightly::with_band_plan(&mut config, band);
    }
    let revision = if config == head.config {
        if state.ours != Some(head.id) {
            info!("nightly RF: the site already holds this plan");
            return Attempt::Done;
        }
        // Its own revision, saved before an apply that couldn't start: apply it now.
        head.id
    } else {
        match site.save(head.id, config, message(&bands)).await {
            Ok(revision) => {
                state.ours = Some(revision);
                revision
            }
            // The next look finds their changes unapplied.
            Err(SaveError::NotLatest) => return Attempt::Wait(NightlySkip::PendingChanges, bands),
            Err(SaveError::Failed(err)) => {
                warn!(%err, "nightly RF: cannot save the plan");
                return Attempt::Done;
            }
        }
    };
    match site.apply(revision).await {
        Ok(job) => {
            state.ours = None;
            info!(%revision, %job, "nightly RF: applying the plan");
            site.note(EventKind::RfOptimized {
                revision,
                job,
                bands,
            });
            Attempt::Done
        }
        Err(StartError::Busy) => Attempt::Wait(NightlySkip::Busy, bands),
        Err(StartError::Failed(err)) => {
            warn!(%err, "nightly RF: cannot start the apply");
            Attempt::Done
        }
    }
}

/// The revision's message, such as `Nightly RF plan: 5 GHz 23 % better`.
fn message(bands: &[BandGain]) -> String {
    let gains: Vec<String> = bands
        .iter()
        .map(|gain| {
            let band = match gain.band {
                Band::G2 => "2.4 GHz",
                Band::G5 => "5 GHz",
                Band::G6 => "6 GHz",
                Band::G60 => "60 GHz",
            };
            format!("{band} {} % better", gain.percent)
        })
        .collect();
    format!("Nightly RF plan: {}", gains.join(", "))
}
