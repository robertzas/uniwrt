//! Client steering (features/wifi.md, Steering & balancing; D-090): usteer on the access points,
//! set by one profile for the site, each WLAN taking part unless it opts out
//! ([`Roaming::steer`](super::wireless::Roaming::steer)). Render writes usteer's package where
//! [`SiteConfig::steers`] holds; the controller's 802.11k neighbor reports leave the steered
//! SSIDs' lists to usteer there, since it sets them itself every second.

use serde::{Deserialize, Serialize};

use super::{
    SiteConfig,
    node::{ManagedPackage, NodeConfig},
    wireless::Wlan,
};
use crate::facts::NodeFacts;

/// Client steering (features/wifi.md, Steering & balancing; D-090): usteer on every access
/// point, set by one profile for the site; each WLAN may opt out ([`Roaming::steer`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Steering {
    pub profile: SteeringProfile,
}

/// How hard usteer pushes clients toward a better access point or band.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SteeringProfile {
    /// No steering: usteer stopped where it is installed.
    #[default]
    Off,
    /// Band steering only, as usteer ships: strong 802.11v clients are asked to move up a band.
    Conservative,
    /// Also moves clients whose signal fades to an access point clearly stronger.
    Balanced,
    /// Moves clients sooner, refuses weak associations, and sheds clients from busy channels.
    Aggressive,
}

impl SteeringProfile {
    pub const ALL: [Self; 4] = [
        Self::Off,
        Self::Conservative,
        Self::Balanced,
        Self::Aggressive,
    ];

    /// What the controller asks of clients under this profile (D-093); `None` when Off.
    pub const fn policy(self) -> Option<SteeringPolicy> {
        match self {
            Self::Off => None,
            Self::Conservative => Some(SteeringPolicy {
                every_s: 120,
                band_above_dbm: -60,
                roam_below_dbm: None,
                margin_db: 0,
            }),
            Self::Balanced => Some(SteeringPolicy {
                every_s: 60,
                band_above_dbm: -63,
                roam_below_dbm: Some(-75),
                margin_db: 8,
            }),
            Self::Aggressive => Some(SteeringPolicy {
                every_s: 30,
                band_above_dbm: -66,
                roam_below_dbm: Some(-72),
                margin_db: 5,
            }),
        }
    }
}

/// The transition requests the controller sends under a profile (features/wifi.md, Steering &
/// balancing; D-093), to 802.11v clients of the steered WLANs, each client asked at most once
/// per `every_s`:
/// - **band steering**: a client heard above `band_above_dbm` on 2.4 GHz for `every_s` is asked
///   to its access point's node's 5 or 6 GHz BSSes of its SSID;
/// - **roam suggestions**: a client below `roam_below_dbm` is asked to the access point usteer
///   hears it best at, if that is at least `margin_db` stronger (and not below the trigger
///   itself), and disconnected 10 s later unless it moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SteeringPolicy {
    pub every_s: u32,
    pub band_above_dbm: i16,
    pub roam_below_dbm: Option<i16>,
    pub margin_db: i16,
}

impl SiteConfig {
    /// The WLANs usteer steers on a node: on, taking part, in scope and on a band the node has
    /// a radio of.
    pub fn steered_wlans(&self, config: &NodeConfig, facts: &NodeFacts) -> Vec<&Wlan> {
        self.wlans
            .iter()
            .filter(|w| w.enabled && w.roaming.steer && w.nodes.holds(config.node))
            .filter(|w| facts.radios.iter().any(|r| w.bands.contains(&r.band)))
            .collect()
    }

    /// Whether usteer runs on the node: steering is on, the node manages usteer, and it
    /// carries a steered WLAN.
    pub fn steers(&self, config: &NodeConfig, facts: &NodeFacts) -> bool {
        self.steering.profile != SteeringProfile::Off
            && config.managed.contains(&ManagedPackage::Usteer)
            && !self.steered_wlans(config, facts).is_empty()
    }

    /// The SSIDs usteer steers wherever it runs (its `ssid_list`), sorted.
    pub fn steered_ssids(&self) -> Vec<&str> {
        let mut ssids: Vec<&str> = self
            .wlans
            .iter()
            .filter(|w| w.enabled && w.roaming.steer)
            .map(|w| w.ssid.as_str())
            .collect();
        ssids.sort_unstable();
        ssids.dedup();
        ssids
    }
}
