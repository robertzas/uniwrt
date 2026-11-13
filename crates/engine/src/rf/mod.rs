//! RF insight (features/wifi.md, RF insight; D-064): the radios' neighbor scans and the report
//! built from them (`uniwrt_rf`).
//!
//! Background scans run each radio every `interval_h` hours (the `rf` setting), only while it is
//! idle (its stations moved under `idle_kbps` in the two latest samples), else again after
//! `retry_busy`; a radio first seen is due after `first_delay`, each further one `stagger`
//! later. Admins queue scans at any time ([`Rf::scan`]); those run first and regardless of
//! traffic or the setting. One scan runs at a time across the site, `gap` apart, through the
//! node's link (the agent runs it in a process of its own). Scans are stored through `RfRepo`
//! and kept 30 days; the report is built again from the nodes' facts, each radio's latest scan
//! and the radar history (the `dfs-radar` log events of 30 days) at most every `rebuild_after`,
//! and announced as [`Event::Rf`] when it changed.
//!
//! [`Rf::plan`] runs the RF planner (features/wifi.md, RF planner; M6.1) on what the scanner
//! holds (the facts, the report, each radio's latest scan, the DFS policy of the settings) and
//! the site's radio policy and overrides, off the async threads.

mod radios;
mod task;

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};

use tokio::{
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
};
use uniwrt_core::NodeId;
use uniwrt_model::{
    facts::NodeFacts,
    node::NodeMode,
    rf::{RadioScan, RfSettings, Scan},
    site::SiteConfig,
};
use uniwrt_node::NodeLink;
use uniwrt_rf::{
    RfView,
    plan::{Plan, Sources},
};
use uniwrt_store::{EventRepo, RfRepo, SettingsRepo};

use self::task::Task;
use crate::Engine;

#[derive(Debug, Clone)]
pub struct RfConfig {
    pub tick_every: Duration,
    pub first_delay: Duration,
    pub stagger: Duration,
    pub retry_busy: Duration,
    pub retry_failed: Duration,
    pub retry_unsupported: Duration,
    /// The least time between two scans.
    pub gap: Duration,
    pub idle_kbps: u64,
    /// How long a scan may take, the agent's own timeout included.
    pub timeout: Duration,
    /// How long scans (and the radar history) are kept.
    pub keep: Duration,
    pub rebuild_after: Duration,
}

impl Default for RfConfig {
    fn default() -> Self {
        Self {
            tick_every: Duration::from_secs(15),
            first_delay: Duration::from_mins(2),
            stagger: Duration::from_mins(1),
            retry_busy: Duration::from_mins(10),
            retry_failed: Duration::from_mins(30),
            retry_unsupported: Duration::from_hours(24),
            gap: Duration::from_secs(30),
            idle_kbps: 500,
            timeout: Duration::from_secs(30),
            keep: Duration::from_hours(30 * 24),
            rebuild_after: Duration::from_secs(5),
        }
    }
}

/// The scanner stopped, or the request named no radio it knows.
#[derive(Debug, thiserror::Error)]
pub enum RfError {
    #[error("RF insight is not running")]
    Stopped,
    #[error("no radio matches")]
    NoRadio,
    #[error("the settings cannot be stored: {0}")]
    Store(String),
    #[error("the planner failed: {0}")]
    Planner(String),
}

enum Command {
    Scan {
        node: Option<NodeId>,
        radio: Option<String>,
        done: oneshot::Sender<Result<Vec<RadioScan>, RfError>>,
    },
    Settings {
        settings: RfSettings,
        done: oneshot::Sender<Result<(), RfError>>,
    },
    Snapshot {
        done: oneshot::Sender<Snapshot>,
    },
    Busy {
        max_kbps: u64,
        done: oneshot::Sender<BTreeSet<(NodeId, String)>>,
    },
}

/// What a plan is made from, as the scanner holds it.
struct Snapshot {
    facts: Vec<(NodeId, Arc<NodeFacts>)>,
    scans: Vec<Scan>,
    view: Arc<RfView>,
}

/// The API's way to RF insight. Clones share it.
#[derive(Clone)]
pub struct Rf {
    commands: mpsc::Sender<Command>,
    view: watch::Receiver<Arc<RfView>>,
}

impl std::fmt::Debug for Rf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Rf").finish_non_exhaustive()
    }
}

impl Rf {
    /// The latest report, scanning states and settings.
    pub fn view(&self) -> Arc<RfView> {
        Arc::clone(&self.view.borrow())
    }

    /// Queues scans of every radio, a node's, or one of them; answers the radios queued.
    pub async fn scan(
        &self,
        node: Option<NodeId>,
        radio: Option<String>,
    ) -> Result<Vec<RadioScan>, RfError> {
        let (done, answer) = oneshot::channel();
        self.commands
            .send(Command::Scan { node, radio, done })
            .await
            .map_err(|_| RfError::Stopped)?;
        answer.await.map_err(|_| RfError::Stopped)?
    }

    /// Plans the site's radios from the latest facts, report and scans, the settings' DFS policy,
    /// `site`'s radio policy and overrides and each node's mode (a node `modes` lacks is taken
    /// as observed: the plan keeps its radios as they run).
    pub async fn plan(
        &self,
        site: SiteConfig,
        modes: BTreeMap<NodeId, NodeMode>,
    ) -> Result<Plan, RfError> {
        let (done, answer) = oneshot::channel();
        self.commands
            .send(Command::Snapshot { done })
            .await
            .map_err(|_| RfError::Stopped)?;
        let snapshot = answer.await.map_err(|_| RfError::Stopped)?;
        tokio::task::spawn_blocking(move || {
            let nodes: Vec<(NodeId, &NodeFacts, NodeMode)> = snapshot
                .facts
                .iter()
                .map(|(node, facts)| {
                    let mode = modes.get(node).copied().unwrap_or(NodeMode::Observe);
                    (*node, &**facts, mode)
                })
                .collect();
            let gathered = uniwrt_rf::plan::gather(&Sources {
                nodes: &nodes,
                report: &snapshot.view.report,
                scans: &snapshot.scans,
                site: &site,
                dfs: snapshot.view.settings.dfs,
            });
            uniwrt_rf::plan::plan(&gathered.inputs())
        })
        .await
        .map_err(|err| RfError::Planner(err.to_string()))
    }

    /// The radios whose stations moved `max_kbps` or more in one of the two latest samples (the
    /// nightly optimizer's heavy traffic).
    pub async fn busy(&self, max_kbps: u64) -> Result<BTreeSet<(NodeId, String)>, RfError> {
        let (done, answer) = oneshot::channel();
        self.commands
            .send(Command::Busy { max_kbps, done })
            .await
            .map_err(|_| RfError::Stopped)?;
        answer.await.map_err(|_| RfError::Stopped)
    }

    /// Stores the settings and applies them.
    pub async fn set_settings(&self, settings: RfSettings) -> Result<(), RfError> {
        let (done, answer) = oneshot::channel();
        self.commands
            .send(Command::Settings { settings, done })
            .await
            .map_err(|_| RfError::Stopped)?;
        answer.await.map_err(|_| RfError::Stopped)?
    }
}

/// Scans and reports until stopped.
pub struct RfScanner {
    stop: oneshot::Sender<()>,
    task: JoinHandle<()>,
    handle: Rf,
}

impl RfScanner {
    pub async fn start<L, R>(engine: &Engine<L>, repo: R, config: RfConfig) -> Self
    where
        L: NodeLink,
        R: RfRepo + EventRepo + SettingsRepo + Clone + 'static,
    {
        let (view, watched) = watch::channel(Arc::new(RfView::default()));
        let bus = engine.subscribe();
        let task = Task::new(engine.clone(), repo, config, view).await;
        let (commands, received) = mpsc::channel(8);
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(task.run(bus, received, stopped));
        Self {
            stop,
            task,
            handle: Rf {
                commands,
                view: watched,
            },
        }
    }

    pub fn handle(&self) -> Rf {
        self.handle.clone()
    }

    pub async fn stop(self) {
        let _ = self.stop.send(());
        let _ = self.task.await;
    }
}

#[cfg(test)]
mod tests;
