//! The controller's automatic install (features/lifecycle.md, Firmware; M7.1b2, D-123): with
//! `auto` on in the `controller_updates` setting, the nightly window (02:30 site time for an
//! hour by default, before the RF optimizer's) installs the newest release published at least
//! a day ago ([`SOAK_S`]) through the [`Updater`], once a night. It waits while a job runs
//! (looking again every ten minutes within the window) and skips the night when GitHub can't
//! be read by its end. A release that fails its trial is rejected by the launcher, so the next
//! night takes nothing older than what runs and nothing rejected.

use std::{sync::Arc, time::Duration};

use tokio::{sync::watch, task::JoinHandle};
use tracing::{info, warn};
use uniwrt_api::controller_update::{self, SelfUpdate as _};
use uniwrt_core::{Clock, Timestamp};
use uniwrt_firmware::{CONTROLLER_REPO, ControllerRelease, GitHub, Releases, Version};
use uniwrt_jobs::Jobs;
use uniwrt_model::{
    controller::{SOAK_S, WINDOW_MIN},
    job::JobState,
    site::tz,
};
use uniwrt_store::{RevisionRepo as _, Store};

use crate::{build_info, launcher::Updater};

/// How often it looks whether the window is open.
const LOOK_EVERY: Duration = Duration::from_mins(10);

/// The task, stopped with [`AutoUpdate::stop`].
pub(crate) struct AutoUpdate(JoinHandle<()>, watch::Sender<bool>);

struct Ctx {
    updater: Updater,
    github: GitHub,
    store: Store,
    jobs: Jobs,
    clock: Arc<dyn Clock>,
}

impl AutoUpdate {
    pub(crate) fn start(
        updater: Updater,
        github: GitHub,
        store: &Store,
        jobs: &Jobs,
        clock: &Arc<dyn Clock>,
    ) -> Self {
        let ctx = Ctx {
            updater,
            github,
            store: store.clone(),
            jobs: jobs.clone(),
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
    /// One look: in the window of a night not yet `done`, installs what [`pick`] finds.
    async fn look(&self, done: &mut Option<i64>) {
        let settings = match controller_update::settings(&self.store).await {
            Ok(settings) => settings,
            Err(err) => return warn!(%err, "automatic install: cannot read the setting"),
        };
        if !settings.auto {
            return;
        }
        let now = self.clock.now();
        let Some(night) = tz::daily_window(
            now,
            site_offset(&self.store, now).await,
            settings.at_min,
            WINDOW_MIN,
        ) else {
            return;
        };
        if *done == Some(night) || self.updater.progress().busy() {
            return;
        }
        if let Some(why) = self.updater.blocked() {
            *done = Some(night);
            return info!(%why, "automatic install: skipped");
        }
        if self.job_running().await {
            return info!("automatic install: waiting for the running job");
        }
        let releases = match self.github.releases(CONTROLLER_REPO, None).await {
            Ok(releases) => releases,
            Err(err) => return warn!(%err, "automatic install: cannot read uniwrt's releases"),
        };
        *done = Some(night);
        let Ok(current) = build_info::VERSION.parse::<Version>() else {
            return;
        };
        let rejected = self.updater.trials().rejected;
        match pick(&releases, build_info::TARGET, current, &rejected, now) {
            None => info!(%current, "automatic install: no newer release out for a day"),
            Some(release) => {
                info!(version = %release.version, "automatic install: installing");
                if let Err(why) = self.updater.install(release) {
                    warn!(%why, "automatic install: cannot install");
                }
            }
        }
    }

    async fn job_running(&self) -> bool {
        self.jobs
            .jobs(20)
            .await
            .is_ok_and(|jobs| jobs.iter().any(|job| job.state == JobState::Running))
    }
}

/// The site's offset from UTC at `now`, in seconds east (UTC without a site): where the
/// nightly windows open.
pub(crate) async fn site_offset(store: &Store, now: Timestamp) -> i32 {
    match store.head_revision().await {
        Ok(Some(head)) => tz::posix(&head.config.site.timezone)
            .and_then(|posix| tz::local(posix, now))
            .map_or(0, |local| local.offset_s),
        Ok(None) => 0,
        Err(err) => {
            warn!(%err, "cannot read the site's time zone");
            0
        }
    }
}

/// The newest release for `target` newer than `current`, not `rejected`, published at least
/// [`SOAK_S`] before `now`, and, from 1.0 on, of `current`'s major version: the next major may
/// break the compatibility promise (D-152), so only a person installs it.
fn pick(
    releases: &Releases,
    target: &str,
    current: Version,
    rejected: &[Version],
    now: Timestamp,
) -> Option<ControllerRelease> {
    let soaked = now.saturating_sub(Duration::from_secs(SOAK_S));
    releases.controller_where(target, |release| {
        release.version > current
            && (current.0 == 0 || release.version.0 == current.0)
            && !rejected.contains(&release.version)
            && release.published_at.is_some_and(|at| at <= soaked)
    })
}

#[cfg(test)]
mod tests {
    use uniwrt_firmware::testing::FakeReleases;

    use super::*;

    const TARGET: &str = "x86_64-unknown-linux-musl";

    #[tokio::test]
    async fn the_newest_release_out_for_a_day_is_picked() {
        let fake = FakeReleases::start().await;
        fake.publish("1.2.2", TARGET, b"older");
        fake.publish("1.2.4", TARGET, b"newer");
        fake.publish("1.2.5", TARGET, b"newest");
        let github = GitHub::new(fake.url(), "uniwrt/test").unwrap();
        let releases = github.releases(CONTROLLER_REPO, None).await.unwrap();
        // The fake publishes every release at 2026-11-16T12:00:00Z.
        let published = Timestamp::parse_utc("2026-11-16T12:00:00Z").unwrap();
        let hours = |h: u64| published.saturating_add(Duration::from_secs(h * 3_600));
        let current = Version(1, 2, 3);
        let picked = |now, rejected: &[Version]| {
            pick(&releases, TARGET, current, rejected, now).map(|release| release.version)
        };
        assert_eq!(picked(hours(23), &[]), None);
        assert_eq!(picked(hours(24), &[]), Some(Version(1, 2, 5)));
        assert_eq!(
            picked(hours(25), &[Version(1, 2, 5)]),
            Some(Version(1, 2, 4))
        );
        assert_eq!(
            picked(hours(25), &[Version(1, 2, 5), Version(1, 2, 4)]),
            None
        );
        assert_eq!(
            pick(&releases, TARGET, Version(1, 2, 5), &[], hours(25)),
            None
        );
    }

    #[tokio::test]
    async fn from_1_0_the_next_major_is_left_to_a_person() {
        let fake = FakeReleases::start().await;
        fake.publish("1.0.0", TARGET, b"one");
        fake.publish("1.4.0", TARGET, b"one later");
        fake.publish("2.0.0", TARGET, b"two");
        let github = GitHub::new(fake.url(), "uniwrt/test").unwrap();
        let releases = github.releases(CONTROLLER_REPO, None).await.unwrap();
        let later = Timestamp::parse_utc("2026-11-20T00:00:00Z").unwrap();
        let picked = |current| pick(&releases, TARGET, current, &[], later).map(|r| r.version);
        assert_eq!(picked(Version(1, 2, 0)), Some(Version(1, 4, 0)));
        assert_eq!(picked(Version(1, 4, 0)), None);
        assert_eq!(picked(Version(2, 0, 0)), None);
        // 0.x has no promise to keep: it moves to the newest.
        assert_eq!(picked(Version(0, 40, 0)), Some(Version(2, 0, 0)));
    }
}
