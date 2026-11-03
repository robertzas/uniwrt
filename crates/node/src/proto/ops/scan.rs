//! `scan`: a neighbor scan of one radio, mirrored by `node-agent/scan.uc`. The answer is raw;
//! [`facts::scan::heard`](crate::facts::scan::heard) parses it.

use serde::{Deserialize, Serialize};

use super::Op;

/// Scans the channels `freqs` (all of the phy's when empty) through `ifname`, one of the radio's
/// interfaces, as an access point may scan (nl80211's AP flag): the radio leaves its channel
/// briefly for each channel scanned. The agent gives up after `timeout_ms` ([`code::TIMEOUT`])
/// and answers [`code::UNSUPPORTED`] when the driver cannot scan there and `busy` as the code of
/// [`code::FAILED`]'s message when another scan is running.
///
/// [`code::TIMEOUT`]: super::code::TIMEOUT
/// [`code::UNSUPPORTED`]: super::code::UNSUPPORTED
/// [`code::FAILED`]: super::code::FAILED
#[derive(Debug, Clone, Serialize)]
pub struct Scan {
    pub ifname: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub freqs: Vec<u32>,
    pub timeout_ms: u64,
}

impl Scan {
    pub const DEFAULT_TIMEOUT_MS: u64 = 20_000;

    pub fn new(ifname: impl Into<String>, freqs: Vec<u32>) -> Self {
        Self {
            ifname: ifname.into(),
            freqs,
            timeout_ms: Self::DEFAULT_TIMEOUT_MS,
        }
    }
}

impl Op for Scan {
    const NAME: &'static str = "scan";
    type Output = Scanned;
}

/// The agent's answer: the scan results (nl80211 `GET_SCAN`) and the phy's channel survey before
/// and after the scan, whose difference is what each channel looked like while it listened.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Scanned {
    /// On the node's clock, in ms.
    pub started_ms: i64,
    pub ended_ms: i64,
    #[serde(default)]
    pub bss: Vec<RawBss>,
    #[serde(default)]
    pub survey_before: Vec<RawSurvey>,
    #[serde(default)]
    pub survey_after: Vec<RawSurvey>,
}

/// One scan result as nl80211 names it, its information elements in base64.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RawBss {
    pub bssid: String,
    pub frequency: u32,
    pub signal_mbm: Option<i32>,
    pub capability: Option<u16>,
    pub seen_ms_ago: Option<i64>,
    pub ies: Option<String>,
}

/// One channel's survey counters (nl80211 `survey_info`), times in ms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct RawSurvey {
    pub frequency: u32,
    pub noise: Option<i32>,
    pub time: Option<u64>,
    pub busy: Option<u64>,
}
