//! Board identity, the firmware release and resource usage (`system board`, `system info`).

use serde::{Deserialize, Serialize};
use uniwrt_core::{MacAddr, Timestamp};

/// What the node is: hardware, kernel and firmware. Changes only with a reflash or upgrade.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct BoardFacts {
    pub hostname: String,
    /// Marketing name, such as `GL.iNet GL-MT6000`.
    pub model: String,
    /// The board identifier that firmware images target, such as `glinet,gl-mt6000`.
    pub board_name: String,
    /// CPU or SoC description, such as `ARMv8 Processor rev 4`.
    pub cpu: String,
    pub kernel: String,
    /// Root filesystem type, such as `squashfs`.
    pub rootfs: Option<String>,
    /// The MAC printed on the label (`board.json`), when the board declares one.
    pub label_mac: Option<MacAddr>,
    pub release: Release,
}

/// The firmware release (`/etc/openwrt_release`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Release {
    /// `OpenWrt`, or the name of a derivative.
    pub distribution: String,
    /// `25.12.0`, `25.12.0-rc2` or `SNAPSHOT`.
    pub version: String,
    /// Source revision, such as `r32064-11f8b4e2b9`.
    pub revision: String,
    /// Target and subtarget, such as `mediatek/filogic`.
    pub target: String,
    /// Package architecture, such as `aarch64_cortex-a53`.
    pub arch: String,
}

/// Resource usage at collection time.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SystemFacts {
    pub uptime_s: u64,
    /// The node's own clock at collection time, for skew detection.
    pub node_time: Option<Timestamp>,
    /// Load averages over 1, 5 and 15 minutes.
    pub load: [f32; 3],
    /// CPU busy percentage since the previous collection; unknown on the first.
    pub cpu_percent: Option<f32>,
    pub memory: Memory,
    /// The writable root (overlay) filesystem.
    pub root: Option<Storage>,
    /// `/tmp` (RAM).
    pub tmp: Option<Storage>,
    /// The hottest thermal sensor, in °C.
    pub temperature_c: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Memory {
    pub total_bytes: u64,
    /// Memory available to new processes without swapping (`MemAvailable`).
    pub available_bytes: u64,
    pub free_bytes: u64,
    pub cached_bytes: u64,
    pub buffered_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Storage {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
}
