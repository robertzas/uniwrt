//! SQM's autotune (features/wan.md, SQM; D-076): with autotune on in the latest revision, a
//! test of the line (SQM bypassed) once a week in the site's night, kept in the history. When
//! it suggests rates more than 10 % away from the site's, it notes an event; the Internet page
//! offers the new rates, which the user reviews and applies. Nothing changes by itself.

use std::{sync::Arc, time::Duration};

use tokio::{sync::watch, task::JoinHandle};
use tracing::{info, warn};
use uniwrt_core::{Clock, Timestamp};
use uniwrt_engine::Engine;
use uniwrt_jobs::{Jobs, SpeedTestRequest};
use uniwrt_model::{
    events::{EventKind, NewEvent},
    job::JobState,
    site::tz,
    speed::{SpeedTests, autotune_due},
};
use uniwrt_store::{RevisionRepo as _, SpeedTestRepo as _, Store};

use crate::fleet_link::FleetLink;

/// How often it looks whether a test is due.
const LOOK_EVERY: Duration = Duration::from_mins(10);
/// How long a test may take before it stops waiting for it.
const TEST_TIMEOUT: Duration = Duration::from_mins(3);

pub(crate) struct Autotune {
    task: JoinHandle<()>,
    stop: watch::Sender<bool>,
}

impl Autotune {
    pub(crate) fn start(
        store: Store,
        jobs: Jobs,
        engine: Engine<FleetLink>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        let (stop, mut stopped) = watch::channel(false);
        let task = tokio::spawn(async move {
            let mut last_try: Option<Timestamp> = None;
            let mut look = tokio::time::interval(LOOK_EVERY);
            look.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    _ = look.tick() => {}
                    _ = stopped.changed() => return,
                }
                if let Some(tried) = once(&store, &jobs, &engine, &*clock, last_try).await {
                    last_try = Some(tried);
                }
            }
        });
        Self { task, stop }
    }

    pub(crate) async fn stop(self) {
        let _ = self.stop.send(true);
        let _ = self.task.await;
    }
}

/// Runs the test if it is due; when it tried, the time it did.
async fn once(
    store: &Store,
    jobs: &Jobs,
    engine: &Engine<FleetLink>,
    clock: &dyn Clock,
    last_try: Option<Timestamp>,
) -> Option<Timestamp> {
    let head = store.head_revision().await.ok()??;
    let sqm = head.config.wan.sqm.clone().filter(|sqm| sqm.autotune)?;
    let offset = tz::posix(&head.config.site.timezone)
        .and_then(tz::standard_offset_s)
        .unwrap_or(0);
    let tests = store.speed_tests(50).await.ok()?;
    let last_test = tests.iter().find(|test| test.bypassed).map(|test| test.at);
    let now = clock.now();
    if !autotune_due(now, offset, last_test, last_try) {
        return None;
    }
    info!("autotune: measuring the line");
    let mut updates = jobs.subscribe();
    let started = match jobs
        .speed_test(SpeedTestRequest { bypass_sqm: true }, None)
        .await
    {
        Ok(job) => job,
        Err(err) => {
            warn!(%err, "autotune: cannot start the speed test");
            return Some(now);
        }
    };
    let ended = tokio::time::timeout(TEST_TIMEOUT, async {
        loop {
            match updates.recv().await {
                Ok(job) if job.id == started.id && job.state != JobState::Running => {
                    return Some(job);
                }
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
            }
        }
    })
    .await
    .ok()
    .flatten();
    if ended.is_none_or(|job| job.state != JobState::Succeeded) {
        return Some(now);
    }
    let latest = store.speed_tests(1).await.ok()?;
    let test = latest
        .first()
        .filter(|test| test.job == started.id)?
        .clone();
    let view = SpeedTests::new(vec![test.clone()], Some(&sqm));
    if let Some(suggestion) = view.suggestion.filter(|s| s.drifted) {
        info!(
            download_kbps = suggestion.download_kbps,
            upload_kbps = suggestion.upload_kbps,
            "autotune: the line moved; suggesting new SQM rates"
        );
        engine.note(
            NewEvent::new(
                clock.now(),
                EventKind::SqmRatesDrifted {
                    job: started.id,
                    download_kbps: suggestion.download_kbps,
                    upload_kbps: suggestion.upload_kbps,
                    shaped_download_kbps: sqm.download_kbps,
                    shaped_upload_kbps: sqm.upload_kbps,
                },
            )
            .node(test.node),
        );
    }
    Some(now)
}
