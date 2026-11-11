//! WLAN schedules (features/wifi.md, WLANs; D-083): the controller turns a WLAN's BSSes off
//! outside its schedule and on again, through each node's agent (the `bss` op: hostapd takes
//! netifd's config less the BSSes that are off, so no configuration changes and nothing runs on
//! the nodes on its own). While the controller is down, a WLAN stays as it is.
//!
//! The saved site (its head revision) says what is wanted, so a schedule needs no apply; the
//! nodes' `radios` facts say what is: a BSS that is off has its section in netifd's status, its
//! interface down. Every `tick_every`, and whenever a node's radios are collected, each managed
//! node's BSSes are compared with what their schedules want, phy by phy ([`plan`]); a phy that
//! differs gets the `bss` op with the BSSes to keep off. A `wifi reload` or an apply brings the
//! BSSes back, and the next comparison turns them off again. Nodes a running job holds are left
//! alone. After acting on a phy it waits `settle` for the facts to catch up, and `retry` before
//! sending the same again (a BSS can be down for other reasons, which the schedule can't fix).
//!
//! Announced as events (`uniwrt_model::events`): `wlan_scheduled` when a schedule turns a WLAN
//! off or on across the site; `wlan_schedule_failed` (raising `node:<id>/wlan-schedule:<phy>`)
//! when a node refuses, and `wlan_schedule_ok` once it takes it.

mod plan;
mod task;

pub(crate) use task::next_job;

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
pub struct SchedulesConfig {
    pub tick_every: Duration,
    /// How long after acting on a phy its facts may still show it as it was.
    pub settle: Duration,
    /// How long before the same request goes to a phy again.
    pub retry: Duration,
    /// How long the `bss` op may take.
    pub timeout: Duration,
}

impl Default for SchedulesConfig {
    fn default() -> Self {
        Self {
            tick_every: Duration::from_secs(30),
            settle: Duration::from_secs(45),
            retry: Duration::from_mins(10),
            timeout: Duration::from_secs(20),
        }
    }
}

/// Keeps the WLANs to their schedules until stopped.
pub struct WlanSchedules {
    stop: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

impl WlanSchedules {
    /// Starts watching; `jobs` tells which nodes a job holds.
    pub fn start<L, R>(
        engine: &Engine<L>,
        repo: R,
        jobs: Option<broadcast::Receiver<Arc<Job>>>,
        config: SchedulesConfig,
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
