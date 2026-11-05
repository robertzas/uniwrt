//! Drift detection (features/lifecycle.md, Drift; D-072, D-074): the agent's hashes of
//! `/etc/config` (the `config` set) against what the controller confirmed on each managed node
//! ([`DriftRepo::confirmed`], written as an apply records). A package whose hash isn't the
//! confirmed one is read over the node's link and diffed with [`uniwrt_uci::diff`]: only a
//! semantic change is drift, and a file rewritten in another form is confirmed as it reads now.
//!
//! Drifts are stored, shown as a [`DriftState`] ([`Drifts::state`]) and announced as
//! [`Event::Drift`], whose changes raise and clear the `node:<id>/drift:<package>` conditions
//! (the event recorder) and so the `config-drift` alert. Users acknowledge a drift (accepted
//! until the file changes again) or mute a node's package. A node is left alone while a job
//! holds it and looked at again once the job ends: an apply confirms its packages anew, which
//! ends their drifts. The tick reads what the store confirmed again and looks at every node,
//! retrying reads that failed.

mod task;
#[cfg(test)]
mod tests;
mod view;

#[cfg(test)]
use view::sha256 as task_sha256;

use std::{sync::Arc, time::Duration};

use tokio::{
    sync::{broadcast, mpsc, oneshot, watch},
    task::JoinHandle,
};
use uniwrt_core::NodeId;
use uniwrt_model::{drift::DriftState, job::Job};
use uniwrt_node::NodeLink;
use uniwrt_store::DriftRepo;

use crate::Engine;

#[derive(Debug, Clone, Copy)]
pub struct DriftConfig {
    /// How often the confirmed packages are read again and every node looked at.
    pub tick_every: Duration,
    /// How long reading one file may take.
    pub read_timeout: Duration,
}

impl Default for DriftConfig {
    fn default() -> Self {
        Self {
            tick_every: Duration::from_mins(1),
            read_timeout: Duration::from_secs(10),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DriftError {
    #[error("drift detection stopped")]
    Stopped,
    #[error("{package} on node {node} hasn't drifted")]
    NotFound { node: NodeId, package: String },
    #[error(transparent)]
    Store(#[from] uniwrt_store::Error),
}

enum Command {
    Acknowledge {
        node: NodeId,
        package: String,
        by: Option<String>,
        done: oneshot::Sender<Result<(), DriftError>>,
    },
    Mute {
        node: NodeId,
        package: String,
        /// Mutes (by the user named), or watches again.
        muted: bool,
        by: Option<String>,
        done: oneshot::Sender<Result<(), DriftError>>,
    },
    /// The files a read task got (internal).
    Read {
        node: NodeId,
        readings: Vec<task::Reading>,
    },
    /// The hashes the agent answered after a job (internal); `None` when it couldn't.
    Hashes {
        node: NodeId,
        hashes: Option<std::collections::BTreeMap<String, String>>,
    },
}

/// The API's handle on drift detection.
#[derive(Clone)]
pub struct Drifts {
    commands: mpsc::Sender<Command>,
    state: watch::Receiver<Arc<DriftState>>,
}

impl std::fmt::Debug for Drifts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Drifts").finish_non_exhaustive()
    }
}

impl Drifts {
    /// Every drift and mute now.
    pub fn state(&self) -> Arc<DriftState> {
        Arc::clone(&self.state.borrow())
    }

    /// Accepts a drift as it is until the file changes again.
    pub async fn acknowledge(
        &self,
        node: NodeId,
        package: String,
        by: Option<String>,
    ) -> Result<(), DriftError> {
        let (done, answer) = oneshot::channel();
        self.send(Command::Acknowledge {
            node,
            package,
            by,
            done,
        })
        .await?;
        answer.await.map_err(|_| DriftError::Stopped)?
    }

    /// Stops watching a node's package (`muted`, by the user named), or watches it again.
    pub async fn mute(
        &self,
        node: NodeId,
        package: String,
        muted: bool,
        by: Option<String>,
    ) -> Result<(), DriftError> {
        let (done, answer) = oneshot::channel();
        self.send(Command::Mute {
            node,
            package,
            muted,
            by,
            done,
        })
        .await?;
        answer.await.map_err(|_| DriftError::Stopped)?
    }

    async fn send(&self, command: Command) -> Result<(), DriftError> {
        self.commands
            .send(command)
            .await
            .map_err(|_| DriftError::Stopped)
    }
}

/// Watches for drift until stopped.
pub struct DriftWatch {
    stop: oneshot::Sender<()>,
    task: JoinHandle<()>,
    handle: Drifts,
}

impl DriftWatch {
    /// Loads what the store holds and starts watching; `jobs` tells which nodes a job holds.
    pub async fn start<L, R>(
        engine: &Engine<L>,
        repo: R,
        jobs: Option<broadcast::Receiver<Arc<Job>>>,
        config: DriftConfig,
    ) -> Result<Self, DriftError>
    where
        L: NodeLink,
        R: DriftRepo + Clone + 'static,
    {
        let bus = engine.subscribe();
        let (commands, received) = mpsc::channel(16);
        let (task, state) =
            task::Task::load(engine.clone(), repo, config, commands.clone()).await?;
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(task.run(bus, jobs, received, stopped));
        Ok(Self {
            stop,
            task,
            handle: Drifts { commands, state },
        })
    }

    pub fn handle(&self) -> Drifts {
        self.handle.clone()
    }

    pub async fn stop(self) {
        let _ = self.stop.send(());
        let _ = self.task.await;
    }
}
