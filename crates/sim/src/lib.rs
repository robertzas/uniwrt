//! The simulated fleet: a gateway, mesh access points and their clients, each node behind a
//! [`NodeLink`](uniwrt_node::NodeLink) that speaks the agent's protocol, for demo mode
//! (`uniwrt serve --demo`) and tests.
//!
//! - **API:** [`SimConfig`] (seed, access points, clients, how often an access point reboots);
//!   [`SimFleet::spawn`] builds the world and runs it, one step per second of tokio's clock;
//!   [`SimFleet::nodes`] describes the nodes as they would be adopted ([`SimNode`]),
//!   [`SimFleet::link`] gives each one's [`SimLink`], and [`SimFleet::reboot`] reboots an access
//!   point on demand.
//! - **The world** (`world/`): nodes at fixed places in a house, clients that sit or walk
//!   between spots and leave the house now and then. Signals follow indoor path loss plus slow
//!   fading; clients prefer 5 GHz while it is good and roam only once their access point is
//!   clearly worse, with 802.11r where they support it; traffic, retries and air time follow
//!   the signal; dnsmasq's leases renew at half time; access points reboot about every
//!   `reboot_every` and their clients move. The gateway reaches one access point by wire and
//!   the others over a batman-adv mesh on 5 GHz, multi-hop where that is better.
//! - **The agent** (`render/`, `link.rs`, `fleet.rs`): every collect set in the raw shapes of
//!   `node-agent/collect/`, so the engine parses them with a real node's parsers;
//!   `subscribe`, `set_interval` and `log_follow` behave as the event-loop agent's (D-036):
//!   hostapd's and dnsmasq's notifications, ticks, and logd batches polled or streamed. A
//!   rebooting node's link goes down, is retried with backoff, and comes back with a new agent.
//!   `exec` runs only what adoption runs (the release, the key, the packages, the backup;
//!   `render/commands.rs`), keeping `authorized_keys` across reboots; `stream` and `upload`
//!   answer `unsupported`.
//! - **Invariants:** the world is a pure function of the seed and the number of steps, with
//!   timestamps counted from the clock's now at spawn; it reads no clock, uses no randomness
//!   but its seeded generator, and iterates in a fixed order. Addresses are documentation or
//!   private ranges, and MACs use real vendors' OUIs only where devices do.
//! - **Dependencies:** `uniwrt-core`, `uniwrt-model` (node roles), `uniwrt-node` (the link
//!   trait and the ops), tokio (the driver, channels), `serde_json`.
//! - **Tests:** unit tests next to the pure parts (generator, radio arithmetic, log lines, the
//!   world's dynamics, every rendered set through the node crate's parsers); `tests/engine.rs`
//!   runs the engine over the fleet on tokio's paused clock (`cargo nextest run -p uniwrt-sim`).

// The simulation is numeric modelling: its floats become the agent's integer fields on purpose.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap
)]

mod fleet;
mod link;
mod render;
mod rng;
mod world;

use std::time::Duration;

pub use fleet::{SimFleet, SimNode};
pub use link::SimLink;

/// What to simulate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimConfig {
    /// The same seed gives the same house, clients and story.
    pub seed: u64,
    pub access_points: usize,
    pub clients: usize,
    /// The mean time between reboots of an access point; `None` for none.
    pub reboot_every: Option<Duration>,
}

impl SimConfig {
    pub const DEFAULT_SEED: u64 = 0x7569_6e77_7274;
}

impl Default for SimConfig {
    /// The reference deployment: a gateway, 3 access points and 30 clients, with a reboot about
    /// every 90 minutes.
    fn default() -> Self {
        Self {
            seed: Self::DEFAULT_SEED,
            access_points: 3,
            clients: 30,
            reboot_every: Some(Duration::from_mins(90)),
        }
    }
}
