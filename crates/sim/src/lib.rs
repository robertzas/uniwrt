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
//!   clearly worse, with 802.11r where they support it (as the world starts, one walks across
//!   the house and is steered over, so a roam between two of the first nodes comes within half
//!   a minute); traffic, retries and air time follow
//!   the signal; dnsmasq's leases renew at half time; access points reboot about every
//!   `reboot_every` and their clients move. The gateway reaches one access point by wire and
//!   the others over a batman-adv mesh on 5 GHz, multi-hop where that is better. A plain cable
//!   joins the first two access points (`world/cables.rs`): bridged at both ends until an apply
//!   brings it over to the wired backhaul, it follows each end's `network` (batman-adv on the
//!   cable's VLAN, the `mtu` op's probe, the port out of the bridge).
//! - **The agent** (`render/`, `link.rs`, `fleet.rs`): every collect set in the raw shapes of
//!   `node-agent/collect/`, so the engine parses them with a real node's parsers;
//!   `subscribe`, `set_interval` and `log_follow` behave as the event-loop agent's (D-036):
//!   hostapd's and dnsmasq's notifications, ticks, and logd batches polled or streamed. A
//!   rebooting node's link goes down, is retried with backoff, and comes back with a new agent.
//!   `exec` runs only what the jobs run (the release, the key, the packages, the backup, the
//!   gateway's `nslookup`; `render/commands.rs`; the feeds' packages SQM and failover need and
//!   their init scripts, `render/feeds.rs`; the speed test's script against a 500/100 Mbit/s
//!   line that bloats unless SQM shapes below it, `line.rs`), and the transaction tool with the node's
//!   `/etc/config` (OpenWrt's stock packages configured by hand for the simulated network,
//!   `render/config.rs`) and init scripts ([`txn::Tool`], which the jobs' tests drive too), keeping
//!   `authorized_keys`, root's password (`demo`; `passwd` changes it) and the tool across
//!   reboots (an open transaction rolls back at boot); the
//!   `config` set hashes those files and `read_file` reads them (drift); `stream` and `upload`
//!   answer `unsupported`. A minute after a node's first confirmed apply, someone edits one of
//!   the packages it wrote by hand (`hand_edit.rs`), once per node, so the demo shows a drift.
//! - **A factory-default gateway** ([`SimConfig::factory`]): root has no password and
//!   `/etc/config` is OpenWrt's own (`render/config.rs`). The LAN follows the gateway's
//!   `network`: it answers at every address of its `lan` ([`SimLink::hosts`]), and when its
//!   ports bounce (the tool's `bounce`, or a rollback's) every host asks for a lease again, so
//!   the nodes, the clients and the controller's own login move to the subnet of `lan`'s first
//!   address (`World::renumber`, D-081). The access point's SSID is the one `wireless` names.
//! - **Invariants:** the world is a pure function of the seed and the number of steps, with
//!   timestamps counted from the clock's now at spawn; it reads no clock, uses no randomness
//!   but its seeded generator, and iterates in a fixed order. Addresses are documentation or
//!   private ranges, and MACs use real vendors' OUIs only where devices do.
//! - **Dependencies:** `uniwrt-core`, `uniwrt-model` (node roles), `uniwrt-node` (the link
//!   trait and the ops), `uniwrt-uci` (the hand edit), tokio (the driver, channels), `serde_json`.
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

mod brownfield;
mod fleet;
mod hand_edit;
mod isp;
mod line;
mod link;
mod render;
mod rng;
pub mod txn;
mod world;

use std::time::Duration;

pub use brownfield::{Brownfield, brownfield};
pub use fleet::{SimFleet, SimNode};
pub use link::SimLink;
pub use render::{
    commands::ROOT_PASSWORD,
    config::{MESH_KEY, PASSPHRASE},
    wifi::{MESH_ID, SSID},
};

/// What to simulate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimConfig {
    /// The same seed gives the same house, clients and story.
    pub seed: u64,
    pub access_points: usize,
    pub clients: usize,
    /// The mean time between reboots of an access point; `None` for none.
    pub reboot_every: Option<Duration>,
    /// The gateway is a factory-default OpenWrt (features/adoption.md, Path E): 192.168.1.1,
    /// root without a password, the stock `/etc/config` with an untagged DHCP WAN and its
    /// radios off, named `OpenWrt`.
    pub factory: bool,
}

impl SimConfig {
    pub const DEFAULT_SEED: u64 = 0x7569_6e77_7274;

    /// A factory-default gateway alone, with a dozen clients behind it and no reboots: what
    /// the first-run wizard sets up.
    pub fn factory(seed: u64) -> Self {
        Self {
            seed,
            access_points: 0,
            clients: 12,
            reboot_every: None,
            factory: true,
        }
    }
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
            factory: false,
        }
    }
}
