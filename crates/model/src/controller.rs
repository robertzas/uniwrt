//! The controller's own update (lifecycle.md, Firmware; D-123): the newest release on offer
//! ([`ControllerUpdate`]), an install's progress ([`InstallProgress`]), the last trial that
//! failed ([`TrialFailure`]) and the automatic install's setting ([`ControllerUpdates`]).

use serde::{Deserialize, Serialize};
use uniwrt_core::Timestamp;

/// The automatic install (the `controller_updates` setting): off by default; on, the newest
/// release published at least [`SOAK_S`] ago is installed in a nightly window of
/// [`WINDOW_MIN`] minutes opening `at_min` after the site's local midnight (02:30 by default,
/// before the RF optimizer's).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default)]
pub struct ControllerUpdates {
    pub auto: bool,
    /// When its window opens, in minutes after local midnight (0–1439).
    pub at_min: u16,
}

impl ControllerUpdates {
    pub const KEY: &'static str = "controller_updates";
}

impl Default for ControllerUpdates {
    fn default() -> Self {
        Self {
            auto: false,
            at_min: 2 * 60 + 30,
        }
    }
}

/// How long the automatic install's window lasts.
pub const WINDOW_MIN: u16 = 60;
/// How long a release must have been out before the automatic install takes it, so a broken
/// release that is quickly replaced is skipped (the Install button offers the newest).
pub const SOAK_S: u64 = 24 * 60 * 60;

/// What `GET v1/controller/update` answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ControllerUpdate {
    /// The running version (`0.0.0-dev…` for a local build).
    pub current: String,
    /// The newest release newer than the running one and not rejected by a failed trial.
    pub offer: Option<ControllerOffer>,
    /// Why GitHub's releases couldn't be read, when they couldn't.
    pub note: Option<String>,
    /// Why this controller can't install a release itself (a development build, demo mode,
    /// self-upgrade off), when it can't.
    pub blocked: Option<String>,
    pub progress: InstallProgress,
    /// The last release that failed its trial (the controller went back to the previous one).
    pub failure: Option<TrialFailure>,
    pub settings: ControllerUpdates,
}

/// A release of the controller on offer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ControllerOffer {
    /// `X.Y.Z`.
    pub version: String,
    pub name: String,
    /// The release's page on GitHub (its notes).
    pub notes: String,
    pub published_at: Option<Timestamp>,
    /// The tarball's size.
    pub size_bytes: u64,
}

/// Where an install stands. It runs in the background: the release downloaded and verified,
/// its binary run once, then the controller restarts into it (a trial the launcher watches).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum InstallProgress {
    #[default]
    Idle,
    /// Downloading the tarball and checking it against GitHub's digest and its attestation.
    Downloading {
        version: String,
    },
    /// Running the new binary once (`version --json`).
    Checking {
        version: String,
    },
    /// Restarting into the new binary.
    Restarting {
        version: String,
    },
    Failed {
        version: String,
        reason: String,
    },
}

impl InstallProgress {
    /// Whether an install is under way.
    pub fn busy(&self) -> bool {
        matches!(
            self,
            Self::Downloading { .. } | Self::Checking { .. } | Self::Restarting { .. }
        )
    }
}

/// A release that failed its trial: not offered or tried again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TrialFailure {
    pub version: String,
    pub reason: String,
    pub at: Timestamp,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_default_to_off_at_half_past_two() {
        let settings: ControllerUpdates = serde_json::from_str("{}").unwrap();
        assert_eq!(settings, ControllerUpdates::default());
        assert!(!settings.auto);
        assert_eq!(settings.at_min, 150);
    }

    #[test]
    fn progress_is_tagged_by_state() {
        let json = serde_json::to_value(InstallProgress::Checking {
            version: "1.2.3".to_owned(),
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({"state": "checking", "version": "1.2.3"})
        );
        assert!(!InstallProgress::Idle.busy());
        assert!(
            InstallProgress::Restarting {
                version: "1.2.3".to_owned()
            }
            .busy()
        );
    }
}
