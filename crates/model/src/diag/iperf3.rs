//! A throughput test's options (D-142): iperf3 between the run's node and a peer, another node
//! (an `iperf3 -s` there for the test's length) or the controller (its own iperf3 server).

use serde::{Deserialize, Serialize};
use uniwrt_core::NodeId;

/// The longest test, in seconds.
pub const MAX_SECONDS: u32 = 60;
/// A test's length when not told, in seconds.
pub const DEFAULT_SECONDS: u32 = 10;
/// The most parallel streams.
pub const MAX_STREAMS: u32 = 8;

pub(super) const fn default_seconds() -> u32 {
    DEFAULT_SECONDS
}

pub(super) const fn default_streams() -> u32 {
    1
}

/// The far end of a throughput test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Iperf3Peer {
    /// The controller, whose iperf3 server listens for the test's node only while it runs.
    Controller,
    /// Another adopted node, which runs `iperf3 -s` for the test.
    Node(NodeId),
}

/// Which way the data flows, seen from the run's node.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Iperf3Direction {
    /// The node sends to the peer (iperf3's default).
    #[default]
    Send,
    /// The peer sends to the node (`-R`).
    Receive,
    /// Both at once (`--bidir`).
    Both,
}

/// Why a test's options can't run, worded for people.
pub(super) fn validate(seconds: u32, streams: u32) -> Result<(), String> {
    if !(1..=MAX_SECONDS).contains(&seconds) {
        return Err(format!(
            "A throughput test lasts 1 to {MAX_SECONDS} seconds"
        ));
    }
    if !(1..=MAX_STREAMS).contains(&streams) {
        return Err(format!("A throughput test runs 1 to {MAX_STREAMS} streams"));
    }
    Ok(())
}
