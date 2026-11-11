//! `rrm`: 802.11k neighbor reports, mirrored by `node-agent/rrm.uc` (features/wifi.md,
//! Roaming).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::Op;

/// Answers each BSS's own neighbor report (only BSSes with 802.11k have one) and gives the
/// BSSes named in `lists`, by interface, their neighbors (hostapd's `rrm_nr_set`, which replaces
/// the list). A BSS that holds the same entries already is left alone, so sending the same lists
/// again is cheap; hostapd forgets them when the BSS restarts.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct Rrm {
    pub lists: BTreeMap<String, Vec<NeighborReport>>,
}

impl Rrm {
    /// The most BSSes and neighbors per BSS one request takes.
    pub const MAX_BSSES: usize = 64;
    pub const MAX_NEIGHBORS: usize = 64;
}

impl Op for Rrm {
    const NAME: &'static str = "rrm";
    type Output = RrmState;
}

/// A neighbor as hostapd takes it: its BSSID (lower case), SSID and report element (lower-case
/// hex), sent as a three-item array.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NeighborReport(pub String, pub String, pub String);

/// The node's BSSes with 802.11k, the lists set (by interface) and those that failed.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RrmState {
    pub own: Vec<OwnReport>,
    pub set: Vec<String>,
    pub failed: Vec<RrmFailure>,
}

/// A BSS's own report.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct OwnReport {
    pub ifname: String,
    pub bssid: String,
    pub ssid: String,
    pub nr: String,
}

impl OwnReport {
    /// The entry a neighbor lists it as.
    pub fn entry(&self) -> NeighborReport {
        NeighborReport(self.bssid.clone(), self.ssid.clone(), self.nr.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RrmFailure {
    pub ifname: String,
    pub error: String,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn lists_go_as_hostapd_takes_them() {
        let entry = NeighborReport("02:00:00:00:01:00".into(), "home".into(), "aabb".into());
        let rrm = Rrm {
            lists: BTreeMap::from([("phy1-ap0".to_owned(), vec![entry])]),
        };
        assert_eq!(
            serde_json::to_value(rrm).unwrap(),
            json!({"lists": {"phy1-ap0": [["02:00:00:00:01:00", "home", "aabb"]]}})
        );
    }
}
