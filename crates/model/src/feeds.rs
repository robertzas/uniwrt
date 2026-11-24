//! The controller's package feed mirror (features/lifecycle.md, Offline packages; D-147): the
//! feed sets it keeps ([`FeedSet`]: one release, target and architecture, whose nodes share
//! their feeds), its settings ([`MirrorSettings`]) and what it holds ([`MirrorStatus`]).

use serde::{Deserialize, Serialize};
use uniwrt_core::Timestamp;

use crate::facts::PackageManager;

/// The feeds the nodes of one release, target and architecture use.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FeedSet {
    /// `24.10.8`, `25.12.6` or `SNAPSHOT`.
    pub release: String,
    /// `mediatek/filogic`.
    pub target: String,
    /// `aarch64_cortex-a53`.
    pub arch: String,
    pub manager: PackageManager,
}

/// The mirror's settings (the `feed_mirror` controller setting).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct MirrorSettings {
    /// Nodes get their packages through the controller (else straight from their feeds).
    pub enabled: bool,
    /// Every package of every feed is kept, not only what the fleet uses (~3 GB a feed set).
    pub whole_feeds: bool,
}

impl MirrorSettings {
    /// The controller setting that holds them.
    pub const KEY: &'static str = "feed_mirror";
}

impl Default for MirrorSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            whole_feeds: false,
        }
    }
}

/// One feed as the mirror holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FeedStatus {
    /// Its base URL upstream.
    pub url: String,
    /// When the index kept was downloaded; `None` while there is none.
    pub index_at: Option<Timestamp>,
    /// When upstream was last asked for the index and answered.
    pub checked_at: Option<Timestamp>,
    /// Why the last refresh failed.
    pub error: Option<String>,
    /// The packages its index lists.
    pub listed: u32,
    /// The packages kept of it, and their bytes.
    pub kept: u32,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub bytes: u64,
}

/// One feed set as the mirror holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SetStatus {
    pub set: FeedSet,
    pub feeds: Vec<FeedStatus>,
    /// The packages kept, and their bytes.
    pub kept: u32,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub bytes: u64,
    /// Wanted names no feed of the set has.
    pub missing: Vec<String>,
    /// Packages that couldn't be downloaded or didn't match their index, with why.
    pub failed: Vec<String>,
}

/// What the mirror holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct MirrorStatus {
    pub sets: Vec<SetStatus>,
    /// Everything kept, in bytes.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub bytes: u64,
    /// When the last sync ended, and what failed in it.
    pub synced_at: Option<Timestamp>,
    pub sync_error: Option<String>,
    /// A sync is running.
    pub syncing: bool,
}
