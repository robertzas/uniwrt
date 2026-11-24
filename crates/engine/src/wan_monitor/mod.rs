//! The WAN quality monitor (features/observability.md, Troubleshooting assistants; D-144): every
//! `every` (30 s), the gateway runs a round of probes (`uniwrt_node::probe`) over its link's
//! exec, not its agent, so a slow round holds nothing up. Each series' round is stored as
//! metric samples (entity `probe:<key>`: the median, fastest and slowest answer and the share
//! lost) and announced as [`Event::Sampled`], so the metric alert rules see them; the round
//! itself is kept for the API ([`WanMonitorHandle::state`]).
//!
//! The settings are the controller's (`WanMonitorSettings::KEY`), read each round, so a save
//! applies at the next one; [`WanMonitorHandle::poke`] runs one at once. What a round says of
//! the internet (`uniwrt_model::wan_monitor::assess`) raises `wan_outage` (`wan-outage`)
//! when every internet target is lost, with the furthest hop a traceroute toward one still
//! reaches, and `wan_restored` once one answers; the fault stays as first placed. A condition
//! the store holds raised at start is resumed. While the gateway is offline or a round fails,
//! nothing is concluded: the gateway's own events tell.

mod task;
#[cfg(test)]
mod tests;

use std::{net::IpAddr, sync::Arc, time::Duration};

use tokio::{
    sync::{Notify, oneshot, watch},
    task::JoinHandle,
};
use uniwrt_core::{NodeId, Timestamp};
use uniwrt_model::wan_monitor::{INTERVAL, ProbeRound, WanHealth, WanMonitorSettings};
use uniwrt_node::NodeLink;
use uniwrt_store::{EventRepo, MetricsRepo, SettingsRepo};

use crate::Engine;

/// The condition an outage raises.
pub const OUTAGE_CONDITION: &str = "wan-outage";

#[derive(Debug, Clone)]
pub struct WanMonitorConfig {
    pub every: Duration,
}

impl Default for WanMonitorConfig {
    fn default() -> Self {
        Self { every: INTERVAL }
    }
}

/// Where the monitor stands.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WanMonitorState {
    pub settings: WanMonitorSettings,
    /// The gateway that ran the latest round.
    pub gateway: Option<NodeId>,
    /// The latest round.
    pub round: Option<ProbeRound>,
    /// What it said of the internet.
    pub health: Option<WanHealth>,
    /// Since when the health is what it is (as far as this controller saw).
    pub since: Option<Timestamp>,
    /// While the internet is down, the furthest hop a traceroute reached.
    pub last_hop: Option<IpAddr>,
    /// Why the latest round didn't run or didn't finish.
    pub error: Option<String>,
}

/// The monitor's task; [`WanMonitor::handle`] reads it.
#[derive(Debug)]
pub struct WanMonitor {
    stop: oneshot::Sender<()>,
    handle: WanMonitorHandle,
    task: JoinHandle<()>,
}

#[derive(Debug, Clone)]
pub struct WanMonitorHandle {
    state: watch::Receiver<Arc<WanMonitorState>>,
    poke: Arc<Notify>,
}

impl WanMonitor {
    pub fn start<L, R>(engine: &Engine<L>, repo: R, config: WanMonitorConfig) -> Self
    where
        L: NodeLink,
        R: SettingsRepo + MetricsRepo + EventRepo + Clone + 'static,
    {
        let (stop, stopped) = oneshot::channel();
        let (state, watched) = watch::channel(Arc::new(WanMonitorState::default()));
        let poke = Arc::new(Notify::new());
        let task = task::Task::new(engine.clone(), repo, config, state, Arc::clone(&poke));
        let task = tokio::spawn(task.run(stopped));
        Self {
            stop,
            handle: WanMonitorHandle {
                state: watched,
                poke,
            },
            task,
        }
    }

    pub fn handle(&self) -> WanMonitorHandle {
        self.handle.clone()
    }

    pub async fn stop(self) {
        let _ = self.stop.send(());
        let _ = self.task.await;
    }
}

impl WanMonitorHandle {
    pub fn state(&self) -> Arc<WanMonitorState> {
        Arc::clone(&self.state.borrow())
    }

    /// Runs a round now (after a save of the settings), or after the one running.
    pub fn poke(&self) {
        self.poke.notify_one();
    }

    /// Waits until the state changes; false once the monitor stopped.
    pub async fn changed(&mut self) -> bool {
        self.state.changed().await.is_ok()
    }
}
