//! 802.11k neighbor reports (features/wifi.md, Roaming): each access point of the managed nodes
//! (those whose saved config manages `wireless`) tells its clients which of the site's other
//! access points broadcast its SSID, so they can measure those first and roam sooner.
//!
//! Through the agent's `rrm` op, the controller learns each node's BSSes with 802.11k (their own
//! reports: BSSID, SSID, channel and capabilities) and gives each BSS its list: the other BSSes
//! of its SSID, its own node's first, then those of the nodes it hears best ([`plan::lists`],
//! over the RF report's matrix), at most [`NeighborsConfig::max_neighbors`]. hostapd keeps lists
//! in memory only, so every `tick_every` each managed node online and not held by a job gets its
//! lists again (the agent leaves alone a BSS that holds them); a node whose own reports changed,
//! a job's end, a node coming or going and a new matrix send the lists that changed at once.
//! Where usteer steers (D-090), it sets the steered SSIDs' lists itself every second, so those
//! BSSes are left to it ([`SiteConfig::steers`]).
//! A node that refuses raises `node:<id>/neighbor-reports`
//! ([`EventKind::NeighborReportsFailed`]), cleared once it takes them
//! ([`EventKind::NeighborReportsOk`]).
//!
//! [`SiteConfig::steers`]: uniwrt_model::site::SiteConfig::steers
//! [`EventKind::NeighborReportsFailed`]: uniwrt_model::events::EventKind::NeighborReportsFailed
//! [`EventKind::NeighborReportsOk`]: uniwrt_model::events::EventKind::NeighborReportsOk

mod plan;
mod task;

use std::{sync::Arc, time::Duration};

use tokio::{
    sync::{broadcast, oneshot},
    task::JoinHandle,
};
use uniwrt_model::job::Job;
use uniwrt_node::NodeLink;
use uniwrt_store::RevisionRepo;

use crate::Engine;

#[derive(Debug, Clone)]
pub struct NeighborsConfig {
    /// How often every managed node gets its lists again.
    pub tick_every: Duration,
    /// How long after a refusal the same node is asked again.
    pub retry: Duration,
    /// How long the `rrm` op may take.
    pub timeout: Duration,
    /// The longest list a BSS gets.
    pub max_neighbors: usize,
}

impl Default for NeighborsConfig {
    fn default() -> Self {
        Self {
            tick_every: Duration::from_mins(1),
            retry: Duration::from_mins(1),
            timeout: Duration::from_secs(15),
            max_neighbors: 16,
        }
    }
}

/// Keeps the lists until stopped.
pub struct NeighborReports {
    stop: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

impl NeighborReports {
    /// Starts keeping them; `jobs` tells which nodes a job holds.
    pub fn start<L, R>(
        engine: &Engine<L>,
        repo: R,
        jobs: Option<broadcast::Receiver<Arc<Job>>>,
        config: NeighborsConfig,
    ) -> Self
    where
        L: NodeLink,
        R: RevisionRepo + Clone + 'static,
    {
        let bus = engine.subscribe();
        let (stop, stopped) = oneshot::channel();
        let task = task::Task::new(engine.clone(), repo, config);
        let task = tokio::spawn(task.run(bus, jobs, stopped));
        Self { stop, task }
    }

    pub async fn stop(self) {
        let _ = self.stop.send(());
        let _ = self.task.await;
    }
}

#[cfg(test)]
mod tests;
