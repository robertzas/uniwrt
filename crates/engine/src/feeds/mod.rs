//! The controller's package feed mirror (features/lifecycle.md, Offline packages; D-147). The
//! copies are the `feeds` crate's ([`Mirror`]); this service decides what it keeps and when it
//! syncs.
//!
//! **Plan** ([`plan::plan`]): one feed set per release, target, architecture and package manager
//! the nodes run (their facts: live, else the stored ones), with the feeds their inventories
//! name (a line the mirror can't keep, such as a local `file:` feed, is left out) and the names
//! wanted of it: every package render may require ([`uniwrt_render::every_package`]), the
//! diagnostic tools ([`OptionalTool`](uniwrt_model::diag::tools::OptionalTool)), the mesh
//! essentials, each node's full wpad where it runs a basic one, and the packages each node has
//! installed on purpose; the mirror adds their dependencies and what nodes fetched through it.
//! A set without a feed is left out. *Keep whole feeds* sets [`Plan::whole`](uniwrt_feeds::Plan).
//! With a [`NextReleases`] ([`FeedMirrorConfig::next`]; the server's reads OpenWrt's catalog),
//! each node's upgrades on offer are planned too (M9.0c1b2): per offer a feed set (`offered`),
//! its feeds derived from the node's ([`uniwrt_feeds::next::derive`]), wanting what uniwrt
//! installs and what the reinstall after OpenWrt's own image would ask for, with that image's
//! files ([`Plan::images`](uniwrt_feeds::Plan)). The lookups are paced and held as [`next`]
//! says; the plan waits for the first answer of each node.
//!
//! **Feeds:** a node whose inventory names none (read before D-147) has them read over its link
//! while it is online, at most once per [`FeedMirrorConfig::read_every`], and kept in its
//! inventory; a watched node without an inventory has them read the same way, held in memory.
//!
//! **Sync**, with the mirror on (the `feed_mirror` setting, [`MirrorSettings`]): a refresh
//! (every index asked for again, conditionally) at start, every
//! [`FeedMirrorConfig::refresh_every`], [`FeedMirrorConfig::retry_after`] after a sync that
//! failed in part, and on demand ([`FeedMirror::refresh`]); a sync without refresh (the indexes
//! of new feeds, the packages of new names, the sweep) whenever the plan changes, which the task
//! looks for every [`FeedMirrorConfig::tick_every`] and when the settings change
//! ([`FeedMirror::reload`]). One sync runs at a time. With the mirror off nothing syncs and
//! nothing is served; the copies stay. [`FeedMirrorConfig::via`] follows the setting, so the
//! nodes' package commands go through the controller only while it serves. No plan is made before the inventories and facts were
//! read, so a start never sweeps the copies for want of them.

mod next;
mod plan;
mod task;

use std::time::Duration;

use tokio::{
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
};
use uniwrt_feeds::{Mirror, ServeError, Served};
use uniwrt_model::feeds::{MirrorSettings, MirrorStatus};
use uniwrt_node::{NodeLink, pkg::via::Via};
use uniwrt_store::{FactsRepo, InventoryRepo, SettingsRepo};

pub use next::{NextReleases, NextSource};
pub use plan::plan;

use crate::Engine;

#[derive(Debug, Clone)]
pub struct FeedMirrorConfig {
    /// How often the inventories, facts and settings are read again.
    pub tick_every: Duration,
    /// How often every index is asked for again.
    pub refresh_every: Duration,
    /// How long after a sync that failed in part the indexes are asked for again.
    pub retry_after: Duration,
    /// How often a node's feeds are read at most, while its inventory names none.
    pub read_every: Duration,
    /// How long that read may take.
    pub timeout: Duration,
    /// How nodes fetch packages through the mirror: on while the mirror runs and its setting
    /// says so (the jobs and the API wrap their package commands with it).
    pub via: Via,
    /// Where the releases nodes are offered come from; `None` keeps only what nodes run.
    pub next: Option<NextSource>,
    /// How often a node's upgrades on offer are looked up at most, while it runs one release.
    pub next_every: Duration,
    /// How long that lookup may take.
    pub next_timeout: Duration,
}

impl Default for FeedMirrorConfig {
    fn default() -> Self {
        Self {
            tick_every: Duration::from_mins(1),
            refresh_every: Duration::from_hours(12),
            retry_after: Duration::from_mins(30),
            read_every: Duration::from_hours(1),
            timeout: Duration::from_secs(30),
            via: Via::default(),
            next: None,
            next_every: Duration::from_hours(1),
            next_timeout: Duration::from_mins(3),
        }
    }
}

/// Why the mirror can't be asked for something.
#[derive(Debug, thiserror::Error)]
pub enum FeedMirrorError {
    #[error("the feed mirror is not running")]
    Stopped,
    #[error("the feed mirror is off")]
    Off,
}

/// Keeps the mirror until stopped.
pub struct FeedMirrorSync {
    stop: oneshot::Sender<()>,
    handle: FeedMirror,
    task: JoinHandle<()>,
}

/// The API's way to the mirror. Clones share it.
#[derive(Clone)]
pub struct FeedMirror {
    mirror: Mirror,
    via: Via,
    commands: mpsc::Sender<task::Command>,
    settings: watch::Receiver<MirrorSettings>,
}

impl FeedMirrorSync {
    /// Starts keeping `mirror` for the nodes `repo` knows.
    pub fn start<L, R>(
        engine: &Engine<L>,
        repo: R,
        mirror: Mirror,
        config: FeedMirrorConfig,
    ) -> Self
    where
        L: NodeLink,
        R: InventoryRepo + FactsRepo + SettingsRepo + 'static,
    {
        let (stop, stopped) = oneshot::channel();
        let (commands, received) = mpsc::channel(8);
        // Off until the setting is read: nothing is served before.
        let off = MirrorSettings {
            enabled: false,
            ..MirrorSettings::default()
        };
        let (settings, watched) = watch::channel(off);
        let via = config.via.clone();
        let task = task::Task::new(engine.clone(), repo, mirror.clone(), config, settings);
        let task = tokio::spawn(task.run(received, stopped));
        Self {
            stop,
            handle: FeedMirror {
                mirror,
                via,
                commands,
                settings: watched,
            },
            task,
        }
    }

    pub fn handle(&self) -> FeedMirror {
        self.handle.clone()
    }

    pub async fn stop(self) {
        let _ = self.stop.send(());
        let _ = self.task.await;
    }
}

impl FeedMirror {
    /// What the mirror holds of the current plan.
    pub fn status(&self) -> MirrorStatus {
        self.mirror.status()
    }

    /// How nodes fetch packages through the mirror, while it is on.
    pub fn via(&self) -> Via {
        self.via.clone()
    }

    /// The settings in force.
    pub fn settings(&self) -> MirrorSettings {
        *self.settings.borrow()
    }

    /// Asks every index again now; returns once the sync is due (it starts after the one
    /// running, if any). [`FeedMirrorError::Off`] while the mirror is off.
    pub async fn refresh(&self) -> Result<(), FeedMirrorError> {
        let (reply, answer) = oneshot::channel();
        self.commands
            .send(task::Command::Refresh { reply })
            .await
            .map_err(|_| FeedMirrorError::Stopped)?;
        answer.await.map_err(|_| FeedMirrorError::Stopped)?
    }

    /// Reads the settings, inventories and facts again now (the settings changed); returns once
    /// they are in force.
    pub async fn reload(&self) -> Result<(), FeedMirrorError> {
        let (reply, answer) = oneshot::channel();
        self.commands
            .send(task::Command::Reload { reply })
            .await
            .map_err(|_| FeedMirrorError::Stopped)?;
        answer.await.map_err(|_| FeedMirrorError::Stopped)
    }

    /// Where the mirror keeps OpenWrt's `image` for an upgrade on offer, when it does.
    pub fn image(
        &self,
        image: &uniwrt_model::firmware::FirmwareImage,
    ) -> Option<std::path::PathBuf> {
        self.mirror.image(image)
    }

    /// A node's request below `/feeds/`; nothing while the mirror is off.
    pub async fn serve(&self, path: &str) -> Result<Served, ServeError> {
        if !self.settings().enabled {
            return Err(ServeError::NotFound);
        }
        self.mirror.serve(path).await
    }
}

#[cfg(test)]
mod tests;
