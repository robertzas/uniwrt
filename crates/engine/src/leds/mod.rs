//! The LEDs schedule (features/network.md, Other services; D-138): the controller turns the
//! managed nodes' LEDs off while the site's `leds_off` schedule is on, and back on after it,
//! through each node's agent (the `leds` op: the agent keeps each LED's trigger and settings in
//! `/tmp` and puts them back), so no configuration changes and nothing runs on the nodes on its
//! own. While the controller is down, the LEDs stay as they are; a node that reboots comes up
//! lit, until the next comparison.
//!
//! The saved site (its head revision) says what is wanted, so the schedule needs no apply; the
//! nodes' `system` facts say what is (how many LEDs are lit, whether the agent holds their
//! state). Every `tick_every`, and whenever a node's `system` set is collected, each managed
//! node that follows the schedule is compared: lit LEDs inside the schedule get `off`, a held
//! state outside it gets `on`. Nodes a running job holds are left alone, and so is every node
//! while the site's time zone can't be read. After acting on a node it waits `settle` for the
//! facts to catch up, and `retry` before sending the same again (an LED some driver keeps lit
//! can't be helped). The schedule turning isn't announced (it is cosmetic, and nightly); a node
//! that refuses raises `led_schedule_failed` (`node:<id>/led-schedule`), cleared by
//! `led_schedule_ok` once it takes it.

mod task;
#[cfg(test)]
mod tests;

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
pub struct LedScheduleConfig {
    pub tick_every: Duration,
    /// How long after acting on a node its facts may still show its LEDs as they were.
    pub settle: Duration,
    /// How long before the same request goes to a node again.
    pub retry: Duration,
    /// How long the `leds` op may take.
    pub timeout: Duration,
}

impl Default for LedScheduleConfig {
    fn default() -> Self {
        Self {
            tick_every: Duration::from_secs(30),
            settle: Duration::from_secs(45),
            retry: Duration::from_mins(10),
            timeout: Duration::from_secs(20),
        }
    }
}

/// Keeps the LEDs to the schedule until stopped.
pub struct LedSchedule {
    stop: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

impl LedSchedule {
    /// Starts watching; `jobs` tells which nodes a job holds.
    pub fn start<L, R>(
        engine: &Engine<L>,
        repo: R,
        jobs: Option<broadcast::Receiver<Arc<Job>>>,
        config: LedScheduleConfig,
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
