//! Client steering's transition requests (features/wifi.md, Steering & balancing; D-093): the
//! controller decides band steering and roam suggestions, since 24.10's and 25.12's hostapd
//! refuse usteer 2022.08's own requests (D-091), and sends them through the agent's `btm` op
//! with the MBO fields those releases need.
//!
//! Every `tick_every`, on each node where usteer runs ([`SiteConfig::steers`]), online and not
//! held by a job, the clients of the steered SSIDs that have 802.11v (the node's hostapd too)
//! are measured against the site profile's [`SteeringPolicy`]; each client is asked at most
//! once per its `every_s`:
//! - **band steering**: heard above `band_above_dbm` on 2.4 GHz through `every_s` (at every
//!   sample of the node's stations), it is asked to its node's 5 and 6 GHz access points of its
//!   SSID (MBO reason: premium AP; nothing follows if it stays);
//! - **roam suggestions**: below `roam_below_dbm`, its node's usteer is asked where else it is
//!   heard (`get_client_info`, from probes across nodes); the access points of its SSID heard
//!   at least `margin_db` stronger (and not below the trigger) are its candidates, best first,
//!   and it is asked with disassociation imminent (MBO reason: low signal): hostapd disconnects
//!   it after [`SteeringConfig::kick_ms`] unless it moved, so it reconnects to a better one.
//!
//! Candidates carry their BSS's own 802.11k report ([`NeighborReports::own_reports`]) with a
//! preference; an access point without 802.11k gives none, and the client chooses. The
//! client's answer comes back as hostapd's `bss-transition-response` push, recorded as a
//! `client_steered` event. Requests stop while the controller is down.
//!
//! [`SiteConfig::steers`]: uniwrt_model::site::SiteConfig::steers
//! [`SteeringPolicy`]: uniwrt_model::site::steering::SteeringPolicy
//! [`NeighborReports::own_reports`]: crate::NeighborReports::own_reports

mod decide;
mod task;

use std::{sync::Arc, time::Duration};

use tokio::{
    sync::{broadcast, oneshot, watch},
    task::JoinHandle,
};
use uniwrt_model::job::Job;
use uniwrt_node::NodeLink;
use uniwrt_store::RevisionRepo;

use crate::{Engine, neighbors::OwnReports};

#[derive(Debug, Clone)]
pub struct SteeringConfig {
    /// How often the clients are measured (the stations' samples come every 15 s, or 2 s while
    /// someone watches).
    pub tick_every: Duration,
    /// How long a `btm` or `ubus` request may take.
    pub timeout: Duration,
    /// How long after a roam suggestion hostapd disconnects a client that stayed (usteer's
    /// `roam_kick_delay`).
    pub kick_ms: u32,
}

impl Default for SteeringConfig {
    fn default() -> Self {
        Self {
            tick_every: Duration::from_secs(5),
            timeout: Duration::from_secs(10),
            kick_ms: 10_000,
        }
    }
}

/// Steers clients until stopped.
pub struct ClientSteering {
    stop: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

impl ClientSteering {
    /// Starts steering; `jobs` tells which nodes a job holds, `own` the access points' own
    /// reports.
    pub fn start<L, R>(
        engine: &Engine<L>,
        repo: R,
        jobs: Option<broadcast::Receiver<Arc<Job>>>,
        own: watch::Receiver<Arc<OwnReports>>,
        config: SteeringConfig,
    ) -> Self
    where
        L: NodeLink,
        R: RevisionRepo + Clone + 'static,
    {
        let (stop, stopped) = oneshot::channel();
        let task = task::Task::new(engine.clone(), repo, own, config);
        let task = tokio::spawn(task.run(jobs, stopped));
        Self { stop, task }
    }

    pub async fn stop(self) {
        let _ = self.stop.send(());
        let _ = self.task.await;
    }
}

#[cfg(test)]
mod tests;
