//! RF insight (features/wifi.md, RF insight): what a radio's neighbor scan heard, the channel
//! survey measured while it listened, and the scanning settings.

use serde::{Deserialize, Serialize};
use uniwrt_core::{MacAddr, NodeId, ScanId, Timestamp};

use crate::wifi::{Band, ChannelWidth, WifiStandard};

/// What one scan of a radio heard: the networks around it and each channel's survey. The `node`
/// crate parses the agent's `scan` answer into this.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Heard {
    pub duration_ms: u32,
    pub bsss: Vec<Bss>,
    /// In frequency order.
    pub channels: Vec<ChannelSurvey>,
}

/// A scan about to be stored.
#[derive(Debug, Clone, PartialEq)]
pub struct NewScan {
    pub node: NodeId,
    /// The radio's UCI section, such as `radio1`.
    pub radio: String,
    pub band: Band,
    /// When it started.
    pub at: Timestamp,
    pub trigger: ScanTrigger,
    pub heard: Heard,
}

/// A stored scan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Scan {
    pub id: ScanId,
    pub node: NodeId,
    pub radio: String,
    pub band: Band,
    pub at: Timestamp,
    pub trigger: ScanTrigger,
    pub duration_ms: u32,
    pub bsss: Vec<Bss>,
    pub channels: Vec<ChannelSurvey>,
}

impl Scan {
    pub fn new(id: ScanId, scan: NewScan) -> Self {
        Self {
            id,
            node: scan.node,
            radio: scan.radio,
            band: scan.band,
            at: scan.at,
            trigger: scan.trigger,
            duration_ms: scan.heard.duration_ms,
            bsss: scan.heard.bsss,
            channels: scan.heard.channels,
        }
    }

    /// The scan without what it heard, for lists.
    pub fn summary(&self) -> ScanSummary {
        ScanSummary {
            id: self.id,
            node: self.node,
            radio: self.radio.clone(),
            band: self.band,
            at: self.at,
            trigger: self.trigger,
            duration_ms: self.duration_ms,
            networks: u32::try_from(self.bsss.len()).unwrap_or(u32::MAX),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ScanSummary {
    pub id: ScanId,
    pub node: NodeId,
    pub radio: String,
    pub band: Band,
    pub at: Timestamp,
    pub trigger: ScanTrigger,
    pub duration_ms: u32,
    /// How many networks it heard.
    pub networks: u32,
}

/// Why a scan ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum ScanTrigger {
    /// The background schedule, while the radio was idle.
    Scheduled,
    /// An admin asked for it.
    Manual,
}

/// A BSS a scan heard: an access point, or a mesh point when `mesh_id` is set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Bss {
    pub bssid: MacAddr,
    /// `None` for a hidden network.
    pub ssid: Option<String>,
    pub mesh_id: Option<String>,
    /// The primary 20 MHz channel.
    pub channel: u16,
    pub frequency_mhz: u32,
    /// The center of the whole channel, which differs from `frequency_mhz` above 20 MHz.
    pub center_frequency_mhz: u32,
    pub width: ChannelWidth,
    pub signal_dbm: i16,
    /// The newest standard it announces.
    pub standard: WifiStandard,
    pub security: Security,
    /// The stations it serves and how busy it finds its channel, when it says (the BSS Load
    /// element).
    pub stations: Option<u16>,
    pub utilization_percent: Option<u8>,
}

/// How a network protects itself, from its RSN and WPA elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum Security {
    Open,
    /// Opportunistic Wireless Encryption (Enhanced Open).
    Owe,
    Wep,
    /// WPA (TKIP) without an RSN element.
    Wpa,
    /// WPA2-Personal (PSK).
    Wpa2,
    /// WPA3-Personal (SAE).
    Wpa3,
    /// WPA2 and WPA3 together (transition mode).
    Wpa2Wpa3,
    /// 802.1X (WPA2- or WPA3-Enterprise).
    Enterprise,
}

/// One channel's survey during a scan.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ChannelSurvey {
    pub channel: u16,
    pub frequency_mhz: u32,
    pub noise_dbm: Option<i16>,
    /// The share of the time the scan listened on it that it was busy.
    pub busy_percent: Option<f32>,
    /// How long the scan listened on it.
    pub dwell_ms: Option<u32>,
}

/// Where a radio's scanning stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct RadioScan {
    pub node: NodeId,
    pub radio: String,
    pub band: Band,
    pub state: ScanState,
    /// The latest scan stored.
    pub last: Option<ScanSummary>,
    /// When the next background scan is due: the schedule's, or a retry's.
    pub next_at: Option<Timestamp>,
    /// Why the latest try failed.
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum ScanState {
    /// Due at `next_at`.
    Scheduled,
    /// Due, but the radio carries traffic: it is tried again at `next_at`.
    Busy,
    /// An admin asked; it runs after the scans before it.
    Queued,
    Scanning,
    /// Background scans are off (manual scans still run).
    Off,
    /// The radio is down, its node offline, or it has no interface to scan through.
    Unavailable,
    /// Its driver cannot scan while it serves (tried again a day later).
    Unsupported,
}

/// The site's scanning settings (setting `rf`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default)]
pub struct RfSettings {
    /// Background scans: each radio every `interval_h` hours, while it is idle (D-064).
    /// Manual scans run either way.
    pub scanning: bool,
    pub interval_h: u8,
    /// Which DFS channels the RF planner may choose (D-110).
    pub dfs: DfsPolicy,
    /// The nightly optimizer (features/wifi.md, RF planner: Apply; D-115): plans every night
    /// and applies the bands that improve by [`NIGHTLY_IMPROVEMENT`] or more.
    pub nightly: bool,
    /// When its window opens, in minutes after local midnight; it lasts [`NIGHTLY_WINDOW_MIN`].
    pub nightly_at_min: u16,
}

impl RfSettings {
    pub const KEY: &'static str = "rf";
    pub const INTERVALS_H: std::ops::RangeInclusive<u8> = 1..=24;
    /// The latest [`Self::nightly_at_min`]: a minute before midnight.
    pub const NIGHTLY_AT_MAX: u16 = 24 * 60 - 1;
}

/// How long the nightly optimizer's window lasts: it tries again within it while traffic is
/// heavy, then waits for the next night.
pub const NIGHTLY_WINDOW_MIN: u16 = 120;
/// The least improvement of a band's cost (`BandPlan::improvement`) the nightly optimizer
/// applies: the hysteresis that keeps it from moving radios for little.
pub const NIGHTLY_IMPROVEMENT: f32 = 0.15;
/// Traffic that puts the nightly apply off: a radio that would move carried this much, in
/// kbit/s, in one of its two latest samples (about a video stream; D-115).
pub const NIGHTLY_HEAVY_KBPS: u64 = 2_000;

/// A band's improvement under the RF planner's plan, in whole percent (the nightly optimizer's
/// events).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct BandGain {
    pub band: Band,
    pub percent: u8,
}

/// Why the nightly optimizer left a plan worth applying for the night (D-115).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum NightlySkip {
    /// The site has changes not yet applied: it never applies what nobody reviewed.
    PendingChanges,
    /// A radio that would move carried heavy traffic until the window closed.
    Traffic,
    /// Another job held the configuration until the window closed.
    Busy,
    /// A managed node was offline until the window closed (an apply needs every one).
    Offline,
}

/// The night whose window holds `now` at a site `offset_s` east of UTC, for a window opening
/// `at_min` minutes after local midnight: the local day it opened on, in days since the epoch.
/// `None` outside the window.
pub fn nightly_window(now: Timestamp, offset_s: i32, at_min: u16) -> Option<i64> {
    let local_min = (now.as_unix_ms().div_euclid(1000) + i64::from(offset_s)).div_euclid(60);
    let into = (local_min - i64::from(at_min)).rem_euclid(24 * 60);
    (into < i64::from(NIGHTLY_WINDOW_MIN)).then(|| (local_min - into).div_euclid(24 * 60))
}

impl Default for RfSettings {
    fn default() -> Self {
        Self {
            scanning: true,
            interval_h: 4,
            dfs: DfsPolicy::All,
            nightly: true,
            nightly_at_min: 3 * 60 + 30,
        }
    }
}

/// Which DFS channels the RF planner may choose (features/wifi.md, RF planner; D-110). A pinned
/// channel is kept whatever the policy says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum DfsPolicy {
    /// No DFS channel.
    Avoid,
    /// DFS channels except the weather radar's (5600–5650 MHz, channels 120–128).
    NonWeather,
    /// Every DFS channel (the owner's default, D-110).
    #[default]
    All,
}

#[cfg(test)]
mod tests {
    use serde_json::{from_value, json, to_value};

    use uniwrt_core::Timestamp;

    use super::{DfsPolicy, RfSettings, ScanTrigger, Security, nightly_window};

    #[test]
    fn serde_names() {
        assert_eq!(to_value(Security::Wpa2Wpa3).unwrap(), json!("wpa2_wpa3"));
        assert_eq!(to_value(ScanTrigger::Manual).unwrap(), json!("manual"));
        let settings: RfSettings = from_value(json!({"scanning": false})).unwrap();
        assert_eq!(
            settings,
            RfSettings {
                scanning: false,
                interval_h: 4,
                dfs: DfsPolicy::All,
                nightly: true,
                nightly_at_min: 210,
            }
        );
        assert_eq!(
            to_value(DfsPolicy::NonWeather).unwrap(),
            json!("non_weather")
        );
    }

    #[test]
    fn the_nightly_window_opens_at_its_local_time_for_two_hours() {
        // Day 20405 at h:m UTC.
        let at =
            |h: i64, m: i64| Timestamp::from_unix_ms((20_405 * 86_400 + h * 3600 + m * 60) * 1000);
        assert_eq!(nightly_window(at(3, 29), 0, 210), None);
        assert_eq!(nightly_window(at(3, 30), 0, 210), Some(20_405));
        assert_eq!(nightly_window(at(5, 29), 0, 210), Some(20_405));
        assert_eq!(nightly_window(at(5, 30), 0, 210), None);
        // Berlin in winter (UTC+1): 03:30 local is 02:30 UTC.
        assert_eq!(nightly_window(at(2, 30), 3600, 210), Some(20_405));
        assert_eq!(nightly_window(at(3, 30), 3600, 210), Some(20_405));
        // New York (UTC−5): 03:30 local is 08:30 UTC.
        assert_eq!(nightly_window(at(8, 45), -5 * 3600, 210), Some(20_405));
        // A window opening at 23:30 runs past midnight and belongs to the day it opened.
        assert_eq!(nightly_window(at(23, 45), 0, 1410), Some(20_405));
        assert_eq!(nightly_window(at(24, 59), 0, 1410), Some(20_405));
        assert_eq!(nightly_window(at(25, 30), 0, 1410), None);
    }
}
