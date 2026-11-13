//! What the planner plans from: the site's radios with their constraints, what each node hears,
//! the channels' history and the policy. The engine fills it from the facts, the report and the
//! site (M6.1 a2); the planner trusts it as given.

use std::collections::BTreeSet;

use uniwrt_core::NodeId;
use uniwrt_model::{
    facts::Channel,
    rf::DfsPolicy,
    site::wireless::TxPower,
    wifi::{Band, ChannelWidth},
};

#[derive(Debug, Clone, Copy)]
pub struct PlanInputs<'a> {
    /// The radios to plan: enabled radios of adopted nodes.
    pub radios: &'a [PlanRadio],
    /// How each node hears each other node, per band (the report's matrix).
    pub links: &'a [Link],
    /// The foreign networks each node hears (the report's networks that aren't the site's own).
    pub heard: &'a [Heard],
    /// What the surveys and the radar history say of each 20 MHz channel.
    pub channels: &'a [ChannelStat],
    pub dfs: DfsPolicy,
}

/// One radio, what it may do and what the site fixes of it.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanRadio {
    pub node: NodeId,
    /// The UCI `wifi-device`, such as `radio1`.
    pub radio: String,
    pub band: Band,
    /// The channels its phy and the regulatory domain allow (`PhyBand::channels`).
    pub channels: Vec<Channel>,
    /// The widths its hardware supports (`PhyBand::widths`).
    pub widths: BTreeSet<ChannelWidth>,
    /// What it runs now; `None` while it is down.
    pub current: Option<Current>,
    /// A channel the site fixes (a hard constraint).
    pub pin: Option<u16>,
    /// A width the site fixes.
    pub pin_width: Option<ChannelWidth>,
    /// `Dbm` fixes the power; `Auto` lets the planner choose it.
    pub tx_power: TxPower,
    /// It carries the site's 802.11s mesh, so it shares one channel with the band's other mesh
    /// radios.
    pub mesh: bool,
    /// The clients it serves: a busier radio's interference counts more.
    pub clients: u16,
    /// Its node is in observe mode (or the site doesn't manage its Wi-Fi): the controller can't
    /// change it, so it comes pinned to what it runs, and its reasons say why.
    pub observed: bool,
}

/// A radio's operating channel and power.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Current {
    /// The primary 20 MHz channel.
    pub channel: u16,
    pub width: ChannelWidth,
    /// The center of the whole channel.
    pub center_frequency_mhz: u32,
    pub tx_power_dbm: Option<i16>,
}

/// Node `from` hears node `to`'s radio on `band` at `signal_dbm`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Link {
    pub band: Band,
    pub from: NodeId,
    pub to: NodeId,
    pub signal_dbm: i16,
    /// A live mesh peer link (the backhaul depends on it).
    pub mesh: bool,
}

/// A foreign network that `node` hears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Heard {
    pub node: NodeId,
    pub band: Band,
    pub primary: u16,
    pub center_frequency_mhz: u32,
    pub width: ChannelWidth,
    pub signal_dbm: i16,
}

/// One 20 MHz channel's history.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChannelStat {
    pub band: Band,
    pub channel: u16,
    /// How busy the latest survey found it (not the live share of a radio on it, which counts
    /// the radio's own traffic).
    pub busy_percent: Option<f32>,
    /// Radar hits on it in the history kept (30 days).
    pub radar_hits: u32,
}
