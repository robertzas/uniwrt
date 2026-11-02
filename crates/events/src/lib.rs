//! Events (features/observability.md, Events, alerts, notifications): deriving the network's
//! timeline from what the engine announces.
//!
//! - **API:** a [`Deriver`] takes what the engine announces, each with the controller's time:
//!   node statuses ([`Deriver::status`]), facts ([`Deriver::facts`]), topology builds
//!   ([`Deriver::topology`]), clients' views ([`Deriver::clients`]), roams, stored log entries
//!   ([`Deriver::logged`]) and jobs, plus a regular [`Deriver::tick`], and returns the
//!   [`NewEvent`]s they make. [`Seed`] is what it knows at start: conditions still raised in the
//!   store, the clients already known and the nodes' last facts. A [`Folder`] plans how to
//!   store events ([`EventWrite`]): repeats of one with a fold key fold into its record.
//! - **What it derives:** nodes offline (after 30 s), degraded (after 60 s), back, rebooted
//!   (their boot time moved) and short of flash (under 10 % free, until 15 %); gateways' WAN
//!   links without a default route (after 10 s) and their addresses changing; mesh links between
//!   adopted nodes down (after 30 s) or weak (quality under 40 for 2 min, until 50); clients
//!   joining, leaving, roaming and new ones; log entries whose pattern is an event; adoptions and
//!   failed jobs. Raised conditions clear with how long they lasted.
//! - **Invariants:** pure and deterministic: no IO and no clock (times come in with the input).
//!   A condition raises once and clears once; nothing is derived about a node that is not
//!   online (its links and clients vanish with it), nor about clients during a node's first
//!   minute online (its first station lists show clients that were already there).
//! - **Dependencies:** `uniwrt-core`, `uniwrt-model`, `uniwrt-topo`, `uniwrt-clients`.
//! - **Tests:** next to each concept, over hand-made inputs (`cargo nextest run -p uniwrt-events`).

mod clients;
mod conditions;
mod derive;
mod fold;
mod links;
#[cfg(test)]
mod tests;

use std::time::Duration;

pub use self::{
    derive::{Deriver, Seed},
    fold::Folder,
};
pub use uniwrt_model::events::{EventWrite, NewEvent};

/// How long states must last, and the thresholds.
#[derive(Debug, Clone, Copy)]
pub struct DeriveConfig {
    pub offline_after: Duration,
    pub degraded_after: Duration,
    pub wan_down_after: Duration,
    pub mesh_down_after: Duration,
    pub mesh_weak_after: Duration,
    /// A mesh link is weak below this quality, and fine again from `mesh_fine_from`.
    pub mesh_weak_below: u8,
    pub mesh_fine_from: u8,
    /// Flash is low below this share free, and fine again from `flash_fine_from`.
    pub flash_low_below: f64,
    pub flash_fine_from: f64,
    /// How long a node is online before its clients' comings and goings are events.
    pub settle: Duration,
    /// How far a node's boot time may move without being a reboot (clock and polling jitter).
    pub reboot_slack: Duration,
    /// The longest log text an event keeps.
    pub max_text: usize,
}

impl Default for DeriveConfig {
    fn default() -> Self {
        Self {
            offline_after: Duration::from_secs(30),
            degraded_after: Duration::from_secs(60),
            wan_down_after: Duration::from_secs(10),
            mesh_down_after: Duration::from_secs(30),
            mesh_weak_after: Duration::from_mins(2),
            mesh_weak_below: 40,
            mesh_fine_from: 50,
            flash_low_below: 0.10,
            flash_fine_from: 0.15,
            settle: Duration::from_mins(1),
            reboot_slack: Duration::from_mins(1),
            max_text: 300,
        }
    }
}

/// Milliseconds, for events' durations.
pub(crate) fn ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}
