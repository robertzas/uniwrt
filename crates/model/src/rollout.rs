//! Rolling firmware upgrades (features/lifecycle.md, Firmware; D-015, D-124, D-125): several
//! nodes upgraded one at a time, leaves first and the gateway last, each by its own upgrade
//! job; an optional canary soaked before the rest; an optional maintenance window outside
//! which no node starts. A rollout is kept as a record, not a job, so that it outlives a
//! controller restart (jobs don't resume, D-042).

use serde::{Deserialize, Serialize};
use uniwrt_core::{JobId, NodeId, RolloutId, Timestamp, UserId};

use crate::site::tz;

/// The canary's soak when none is given, in minutes (D-125).
pub const CANARY_SOAK_MIN: u32 = 60;
/// The shortest and longest canary soak, in minutes.
pub const CANARY_SOAK_RANGE: (u32, u32) = (30, 24 * 60);
/// The window when none is given: 03:30–05:30 site time, after the controller's own install
/// and the RF optimizer (D-125).
pub const WINDOW: UpgradeWindow = UpgradeWindow {
    start_min: 3 * 60 + 30,
    end_min: 5 * 60 + 30,
};
/// The shortest window, in minutes: one node's upgrade takes up to about 20.
pub const WINDOW_MIN_LEN: u16 = 30;

const DAY_S: i64 = 86_400;

/// A rolling upgrade and how far it got.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Rollout {
    pub id: RolloutId,
    pub state: RolloutState,
    /// Every node in the order they upgrade: the canary first, the gateway last.
    pub nodes: Vec<RolloutNode>,
    /// The first node's soak before the rest, if any.
    pub canary: Option<Canary>,
    /// When nodes may start, if not at any time.
    pub window: Option<UpgradeWindow>,
    /// What it waits for between two nodes while it runs.
    pub waiting: Option<RolloutWait>,
    /// Why it halted (a node's failure), or how it was cancelled.
    pub error: Option<String>,
    /// Who started it.
    pub created_by: Option<UserId>,
    pub started_at: Timestamp,
    pub finished_at: Option<Timestamp>,
}

/// A rollout about to start, its nodes in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRollout {
    pub nodes: Vec<RolloutNode>,
    pub canary: Option<Canary>,
    pub window: Option<UpgradeWindow>,
    pub created_by: Option<UserId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum RolloutState {
    /// Upgrading a node, or waiting for the next one's turn ([`Rollout::waiting`]).
    Running,
    /// An admin paused it: the node upgrading finishes, the next doesn't start.
    Paused,
    /// A node failed: nothing more starts until an admin retries it (resume) or skips it.
    Halted,
    /// Every node upgraded or skipped.
    Done,
    /// An admin ended it; the nodes not upgraded yet keep their firmware.
    Cancelled,
}

impl RolloutState {
    /// Whether it may still upgrade a node.
    pub fn active(self) -> bool {
        matches!(self, Self::Running | Self::Paused | Self::Halted)
    }
}

/// One node of a rollout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RolloutNode {
    pub node: NodeId,
    pub name: String,
    /// The image it gets, as the admin chose it from its offers.
    pub choice: UpgradeChoice,
    /// The release it ran when the rollout started.
    pub from: String,
    pub state: RolloutNodeState,
    /// Its upgrade job, once started (the last one, after a retry).
    pub job: Option<JobId>,
    /// What happened to it last, for people.
    pub detail: Option<String>,
}

/// The image to upgrade a node with, as the upgrade dialog plans it (D-122).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
pub enum UpgradeChoice {
    /// Built by ASU, one of the node's offers: asked for when the rollout starts, waited for at
    /// the node's turn.
    Asu { release: String },
    /// OpenWrt's image of `release`, one of the node's offers.
    Official { release: String },
    /// The uploaded image `sha256`.
    Upload { sha256: String },
    /// The release tagged `release` of the node's GitHub repo.
    Github { release: String },
}

impl UpgradeChoice {
    /// The release it installs, when it names one (not an upload's).
    pub fn release(&self) -> Option<&str> {
        match self {
            Self::Asu { release } | Self::Official { release } | Self::Github { release } => {
                Some(release)
            }
            Self::Upload { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum RolloutNodeState {
    Pending,
    /// Its image is being readied (ASU building it, its plan read again).
    Preparing,
    /// Its upgrade job runs.
    Upgrading,
    Upgraded,
    /// Its upgrade failed (or couldn't start): the rollout halted on it.
    Failed,
    /// Left out by an admin, or by a cancel: its firmware is as it was.
    Skipped,
}

/// The canary: the rollout's first node, watched for a while before the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Canary {
    /// How long it soaks once upgraded, in minutes.
    pub soak_min: u32,
}

/// A daily maintenance window in the site's time: nodes start only inside it (one that started
/// finishes after it closes). `end_min` before `start_min` crosses midnight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct UpgradeWindow {
    /// Minutes after local midnight it opens (0–1439).
    pub start_min: u16,
    /// Minutes after local midnight it closes (0–1439).
    pub end_min: u16,
}

impl UpgradeWindow {
    /// How long it stays open, in minutes.
    pub fn len_min(self) -> u16 {
        (self.end_min + 24 * 60 - self.start_min) % (24 * 60)
    }

    /// Why it can't be used, for people.
    pub fn check(self) -> Result<(), String> {
        if self.start_min >= 24 * 60 || self.end_min >= 24 * 60 {
            return Err("A window's times are within a day (00:00–23:59)".to_owned());
        }
        if self.len_min() < WINDOW_MIN_LEN {
            return Err(format!(
                "A window must stay open at least {WINDOW_MIN_LEN} minutes"
            ));
        }
        Ok(())
    }

    /// Whether it is open at `now`, at a site `offset_s` east of UTC.
    pub fn is_open(self, now: Timestamp, offset_s: i32) -> bool {
        tz::daily_window(now, offset_s, self.start_min, self.len_min()).is_some()
    }

    /// When it opens next after `now` (`now` itself when open).
    pub fn opens(self, now: Timestamp, offset_s: i32) -> Timestamp {
        if self.is_open(now, offset_s) {
            return now;
        }
        let local_s = now.as_unix_ms().div_euclid(1000) + i64::from(offset_s);
        let mut opens = local_s - local_s.rem_euclid(DAY_S) + i64::from(self.start_min) * 60;
        if opens <= local_s {
            opens += DAY_S;
        }
        Timestamp::from_unix_secs(opens - i64::from(offset_s))
    }
}

/// What a running rollout waits for before its next node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RolloutWait {
    /// The canary's soak, until then.
    Canary { until: Timestamp },
    /// The maintenance window, opening then.
    Window { opens: Timestamp },
    /// Another job holds the next node (an apply, say): it tries again shortly.
    Busy { why: String },
}

#[cfg(test)]
mod tests {
    use uniwrt_core::Timestamp;

    use super::{UpgradeChoice, UpgradeWindow, WINDOW};

    /// 2026-11-17 `h:m` UTC.
    fn at(h: i64, m: i64) -> Timestamp {
        Timestamp::from_unix_ms((1_763_337_600 + h * 3600 + m * 60) * 1000)
    }

    #[test]
    fn the_default_window_opens_at_half_past_three_site_time() {
        assert!(!WINDOW.is_open(at(3, 29), 0));
        assert!(WINDOW.is_open(at(3, 30), 0));
        assert!(WINDOW.is_open(at(5, 29), 0));
        assert!(!WINDOW.is_open(at(5, 30), 0));
        assert_eq!(WINDOW.opens(at(4, 0), 0), at(4, 0));
        assert_eq!(WINDOW.opens(at(1, 15), 0), at(3, 30));
        assert_eq!(WINDOW.opens(at(6, 0), 0), at(27, 30));
        // An hour east of UTC, 03:30 there is 02:30 here.
        assert_eq!(WINDOW.opens(at(1, 0), 3600), at(2, 30));
        assert!(WINDOW.is_open(at(2, 45), 3600));
    }

    #[test]
    fn a_window_may_cross_midnight_and_must_last_half_an_hour() {
        let night = UpgradeWindow {
            start_min: 23 * 60,
            end_min: 60,
        };
        assert_eq!(night.len_min(), 120);
        assert!(night.check().is_ok());
        assert!(night.is_open(at(0, 30), 0));
        assert!(!night.is_open(at(1, 0), 0));
        let short = UpgradeWindow {
            start_min: 600,
            end_min: 620,
        };
        assert!(short.check().is_err());
        let late = UpgradeWindow {
            start_min: 1440,
            end_min: 60,
        };
        assert!(late.check().is_err());
    }

    #[test]
    fn choices_are_tagged_by_source() {
        let choice: UpgradeChoice =
            serde_json::from_value(serde_json::json!({"source": "asu", "release": "25.12.1"}))
                .unwrap();
        assert_eq!(choice.release(), Some("25.12.1"));
        let upload: UpgradeChoice =
            serde_json::from_value(serde_json::json!({"source": "upload", "sha256": "ab"}))
                .unwrap();
        assert_eq!(upload.release(), None);
    }
}
