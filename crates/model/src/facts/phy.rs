//! Radio hardware (kernel phys): per band, the standards, widths and channels that the hardware
//! and the regulatory domain allow. It changes rarely, so it is collected apart from the radios'
//! operating state.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::wifi::{Band, ChannelWidth, WifiStandard};

/// One kernel phy (wiphy). A phy may serve several bands; the radios on it say which they use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PhyFacts {
    /// Such as `phy0`.
    pub name: String,
    /// In band order.
    pub bands: Vec<PhyBand>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PhyBand {
    pub band: Band,
    pub standards: BTreeSet<WifiStandard>,
    pub widths: BTreeSet<ChannelWidth>,
    /// The most spatial streams of the newest standard.
    pub spatial_streams: Option<u8>,
    /// Usable channels; channels the regulatory domain disables are left out.
    pub channels: Vec<Channel>,
}

impl PhyBand {
    /// The highest power any channel allows.
    pub fn max_tx_power_dbm(&self) -> Option<i16> {
        self.channels
            .iter()
            .filter_map(|c| c.max_tx_power_dbm)
            .max()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Channel {
    pub number: u16,
    pub frequency_mhz: u32,
    /// Radar detection is required before use.
    pub dfs: bool,
    /// Where a DFS channel stands; `None` on other channels.
    pub dfs_state: Option<DfsState>,
    /// No initiating radiation: the radio may not start a BSS on this channel.
    pub no_ir: bool,
    pub max_tx_power_dbm: Option<i16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum DfsState {
    /// Usable after a channel availability check (CAC).
    Usable,
    /// Radar was detected; unusable until the non-occupancy period ends.
    Unavailable,
    /// The CAC passed; usable at once.
    Available,
}
