//! Nightly controller backups (features/lifecycle.md, Backups; D-129): in the window at
//! [`AT_MIN`] site time ([`WINDOW_MIN`] minutes, before the controller's own update and the node
//! backups), once an admin has set the passphrase, one backup of the database and the master key
//! a night ([`ControllerBackups::run`], which records how it went). While one an admin asked for
//! runs, it tries again within the window.

use std::{sync::Arc, time::Duration};

use tokio::{sync::watch, task::JoinHandle};
use tracing::{info, warn};
use uniwrt_core::Clock;
use uniwrt_model::{
    controller_backup::{AT_MIN, WINDOW_MIN},
    site::tz,
};
use uniwrt_store::{Error, Store, Vault, controller_backup::ControllerBackups};

use crate::{auto_update::site_offset, build_info};

/// How often it looks whether the window is open.
const LOOK_EVERY: Duration = Duration::from_mins(5);

/// The task, stopped with [`NightlyBackups::stop`].
pub(crate) struct NightlyBackups(JoinHandle<()>, watch::Sender<bool>);

struct Ctx {
    store: Store,
    vault: Vault,
    backups: ControllerBackups,
    clock: Arc<dyn Clock>,
}

impl NightlyBackups {
    pub(crate) fn start(
        backups: &ControllerBackups,
        store: &Store,
        vault: &Vault,
        clock: &Arc<dyn Clock>,
    ) -> Self {
        let ctx = Ctx {
            store: store.clone(),
            vault: vault.clone(),
            backups: backups.clone(),
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
    /// One look: in the window of a night not yet `done`, takes the backup.
    async fn look(&self, done: &mut Option<i64>) {
        let now = self.clock.now();
        let offset = site_offset(&self.store, now).await;
        let Some(night) = tz::daily_window(now, offset, AT_MIN, WINDOW_MIN) else {
            return;
        };
        if *done == Some(night) {
            return;
        }
        match self
            .backups
            .run(&self.store, &self.vault, build_info::VERSION)
            .await
        {
            Ok(taken) => {
                *done = Some(night);
                info!(
                    name = taken.backup.name,
                    size = taken.backup.size_bytes,
                    "controller backup taken"
                );
                if let Some(err) = taken.copy_error {
                    warn!(err, "controller backup: not copied to the extra directory");
                }
            }
            Err(Error::Conflict { .. }) => info!("controller backup: waiting for the one running"),
            Err(Error::NotFound { .. }) => {
                *done = Some(night);
                warn!(
                    "no controller backup tonight: no backup passphrase is set (Settings → Backups)"
                );
            }
            Err(err) => {
                *done = Some(night);
                warn!(
                    err = format!("{:#}", anyhow::Error::from(err)),
                    "controller backup failed"
                );
            }
        }
    }
}
