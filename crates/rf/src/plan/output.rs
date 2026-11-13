//! The plan: per band, each radio's planned channel, width and power with the cost before and
//! after and why; the band's score before and after and how the search ran.

use serde::{Deserialize, Serialize};
use uniwrt_core::NodeId;
use uniwrt_model::wifi::{Band, ChannelWidth};

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Plan {
    /// In band order.
    pub bands: Vec<BandPlan>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct BandPlan {
    pub band: Band,
    /// By node, then radio.
    pub radios: Vec<RadioPlan>,
    /// The band's cost as it runs and as planned (the planned cost counts the changes).
    pub before: f32,
    pub after: f32,
    /// `(before − after) / before`, 0 when `before` is 0; the nightly optimizer applies a plan
    /// only from 0.15 (M6.1 b3).
    pub improvement: f32,
    pub search: Search,
    /// Every combination was weighed, so no plan scores better under the model.
    pub optimal: bool,
    /// Radios with neither a channel to choose nor a current one, left out.
    pub skipped: Vec<RadioRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Search {
    /// Nothing to choose: every radio is pinned or has one channel.
    Fixed,
    /// Branch and bound over every combination.
    Exhaustive,
    /// Simulated annealing with a fixed seed, then a local descent.
    Annealing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RadioRef {
    pub node: NodeId,
    pub radio: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RadioPlan {
    pub node: NodeId,
    pub radio: String,
    pub current: Option<Setting>,
    pub planned: Setting,
    /// The channel, width or power differs from the current.
    pub changed: bool,
    /// Its cost as it runs (`None` while it is down) and as planned.
    pub before: Option<Breakdown>,
    pub after: Breakdown,
    pub reasons: Vec<Reason>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Setting {
    /// The primary 20 MHz channel.
    pub channel: u16,
    pub width: ChannelWidth,
    pub center_frequency_mhz: u32,
    /// `None` leaves it to the radio (the regulatory maximum).
    pub tx_power_dbm: Option<i16>,
}

/// A radio's cost, term by term (features/wifi.md, RF planner: Objective).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Breakdown {
    /// The site's other radios overlapping it, by how loud its node hears them.
    pub own: f32,
    /// Foreign networks overlapping it, and how busy the surveys found its channels.
    pub external: f32,
    /// DFS: the risk of radar (more with a history of hits), and the CAC's silence on moving.
    pub dfs: f32,
    /// Moving at all.
    pub change: f32,
    /// The width given up below the widest it may use.
    pub width: f32,
    pub total: f32,
}

/// Why a radio is planned as it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Reason {
    /// The site fixes its channel.
    PinnedChannel,
    /// The site fixes its width.
    PinnedWidth,
    /// The site fixes its power.
    PinnedPower,
    /// It carries the mesh, so it takes the channel of the band's `radios` mesh radios.
    MeshGroup { radios: u16 },
    /// Its pin differs from the mesh's, which follows another mesh radio's pin.
    PinConflict { node: NodeId, radio: String },
    /// No channel fits its constraints, so it keeps the one it runs.
    NoChannel,
    /// Narrower than the widest it may use: the wider channels cost more than they bring.
    Narrowed { widest: ChannelWidth },
    /// The loudest other node hears it above −65 dBm: lowered towards it, for roaming's sake.
    PowerLowered { heard_dbm: i16 },
    /// No other node hears it above −72 dBm: raised towards it.
    PowerRaised { heard_dbm: i16 },
    /// Kept up so the mesh peer `peer` still hears it at −70 dBm.
    MeshPower { peer: NodeId, signal_dbm: i16 },
    /// 2.4 GHz kept 3 dB under its node's 5 GHz radio, to bias clients to 5 GHz.
    BelowFiveGhz,
}
