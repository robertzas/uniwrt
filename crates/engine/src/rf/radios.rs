//! Each radio the scanner knows, from its node's facts: how to scan it (an interface and the
//! band's channels), how much traffic it carries, and when it is next due.

use std::collections::{BTreeMap, BTreeSet};

use uniwrt_core::{NodeId, Timestamp};
use uniwrt_model::{
    facts::{NodeFacts, RadioFacts, WifiIfaceMode},
    rf::{RadioScan, ScanState, ScanSummary},
    wifi::Band,
};

/// A radio: its node and UCI section.
pub(super) type Key = (NodeId, String);

/// Traffic samples less than this far apart are not rated.
const MIN_SAMPLE_MS: i64 = 5_000;

#[derive(Debug)]
pub(super) struct Radio {
    pub(super) band: Band,
    pub(super) up: bool,
    /// The interface to scan through: an access point's, else a mesh point's.
    pub(super) ifname: Option<String>,
    /// The band's channels its phy has, in MHz.
    pub(super) freqs: Vec<u32>,
    /// Its stations' bytes, and when they were counted.
    bytes: Option<(u64, Timestamp)>,
    /// The two latest traffic rates, in kbit/s, newest first.
    rates: [Option<u64>; 2],
    pub(super) last: Option<ScanSummary>,
    pub(super) next_at: Timestamp,
    /// `Scheduled`, `Busy` or `Unsupported`: what the schedule last decided.
    pub(super) state: ScanState,
    pub(super) error: Option<String>,
}

impl Radio {
    pub(super) fn new(band: Band, next_at: Timestamp) -> Self {
        Self {
            band,
            up: false,
            ifname: None,
            freqs: Vec::new(),
            bytes: None,
            rates: [None; 2],
            last: None,
            next_at,
            state: ScanState::Scheduled,
            error: None,
        }
    }

    /// Whether it can be scanned now: up, with an interface to scan through.
    pub(super) fn scannable(&self) -> bool {
        self.up && self.ifname.is_some()
    }

    /// Its traffic stayed under `max_kbps` in the two latest samples (or there were none).
    pub(super) fn idle(&self, max_kbps: u64) -> bool {
        self.rates.iter().flatten().all(|&rate| rate < max_kbps)
    }

    fn update(&mut self, radio: &RadioFacts, facts: &NodeFacts) {
        self.band = radio.band;
        self.up = radio.up;
        let up: Vec<_> = radio.ifaces.iter().filter(|i| i.up).collect();
        self.ifname = [WifiIfaceMode::Ap, WifiIfaceMode::Mesh]
            .into_iter()
            .find_map(|mode| up.iter().find(|i| i.mode == mode)?.name.clone());
        self.freqs = facts
            .phys
            .iter()
            .filter(|p| radio.phy.as_ref() == Some(&p.name))
            .flat_map(|p| &p.bands)
            .filter(|b| b.band == radio.band)
            .flat_map(|b| b.channels.iter().map(|c| c.frequency_mhz))
            .collect();
        let names: BTreeSet<&str> = radio
            .ifaces
            .iter()
            .filter_map(|i| i.name.as_deref())
            .collect();
        let total: u64 = facts
            .stations
            .iter()
            .filter(|s| names.contains(s.iface.as_str()))
            .map(|s| s.rx_bytes + s.tx_bytes)
            .sum();
        let at = facts.collected_at;
        match self.bytes {
            Some((before, then)) if at.as_unix_ms() - then.as_unix_ms() < MIN_SAMPLE_MS => {
                // Too soon to rate; a client gone meanwhile makes the next sample the baseline.
                if total < before {
                    self.bytes = Some((total, at));
                }
            }
            Some((before, then)) => {
                let ms = u64::try_from(at.as_unix_ms() - then.as_unix_ms()).unwrap_or(u64::MAX);
                // Fewer bytes than before: a station left and took its counters along.
                let rate = total.checked_sub(before).map(|bytes| bytes * 8 / ms.max(1));
                self.rates = [rate, self.rates[0]];
                self.bytes = Some((total, at));
            }
            None => self.bytes = Some((total, at)),
        }
    }

    pub(super) fn view(&self, key: &Key, state: ScanState) -> RadioScan {
        RadioScan {
            node: key.0,
            radio: key.1.clone(),
            band: self.band,
            state,
            last: self.last.clone(),
            next_at: matches!(
                state,
                ScanState::Scheduled | ScanState::Busy | ScanState::Unsupported
            )
            .then_some(self.next_at),
            error: self.error.clone(),
        }
    }
}

/// Takes in a node's facts: its radios appear (first due at `first_due(i)` for the `i`th new
/// one), change and go.
pub(super) fn update(
    radios: &mut BTreeMap<Key, Radio>,
    node: NodeId,
    facts: &NodeFacts,
    mut first_due: impl FnMut() -> Timestamp,
) {
    let mut seen = BTreeSet::new();
    for radio in facts
        .radios
        .iter()
        .filter(|r| !r.disabled && r.band != Band::G60)
    {
        let key = (node, radio.name.clone());
        seen.insert(radio.name.as_str());
        radios
            .entry(key)
            .or_insert_with(|| Radio::new(radio.band, first_due()))
            .update(radio, facts);
    }
    radios.retain(|(n, name), _| *n != node || seen.contains(name.as_str()));
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;
    use uniwrt_core::{NodeId, Timestamp};
    use uniwrt_model::facts::NodeFacts;

    use super::update;

    fn facts(s: i64, bytes: u64) -> NodeFacts {
        let mut facts = NodeFacts {
            collected_at: Timestamp::from_unix_ms(s * 1000),
            ..NodeFacts::default()
        };
        facts.radios = serde_json::from_value(json!([{
            "name": "radio1", "phy": "phy1", "band": "5g", "up": true, "disabled": false,
            "ifaces": [
                {"name": "phy1-mesh0", "mode": "mesh", "networks": [], "up": true},
                {"name": "phy1-ap0", "mode": "ap", "networks": [], "up": true}
            ]
        }]))
        .unwrap();
        facts.phys = serde_json::from_value(json!([{"name": "phy1", "bands": [{
            "band": "5g", "standards": [], "widths": [], "spatial_streams": null,
            "channels": [{"number": 36, "frequency_mhz": 5180, "dfs": false, "dfs_state": null,
                          "no_ir": false, "max_tx_power_dbm": null}]
        }]}]))
        .unwrap();
        facts.stations = serde_json::from_value(json!([{
            "mac": "02:00:00:00:00:09", "iface": "phy1-ap0", "signal_dbm": -50,
            "connected_s": 1, "inactive_ms": 0, "rx_rate": null, "tx_rate": null,
            "rx_bytes": bytes, "tx_bytes": 0, "rx_packets": 0, "tx_packets": 0,
            "tx_retries": null, "tx_failed": null, "expected_throughput_kbps": null,
            "rx_airtime_us": null, "tx_airtime_us": null, "peer_link": null,
            "authorized": true, "standard": null, "capabilities": []
        }]))
        .unwrap();
        facts
    }

    #[test]
    fn a_radio_scans_through_its_ap_and_is_idle_while_its_traffic_is_low() {
        let mut radios = BTreeMap::new();
        let node = NodeId::new(1);
        let due = Timestamp::from_unix_ms(42);
        update(&mut radios, node, &facts(0, 0), || due);
        let radio = &radios[&(node, "radio1".to_owned())];
        assert_eq!(radio.ifname.as_deref(), Some("phy1-ap0"));
        assert_eq!((radio.freqs.as_slice(), radio.next_at), (&[5180][..], due));
        assert!(radio.scannable() && radio.idle(500));
        // 15 s at 1 MB/s: 533 kbit/s, then quiet.
        update(&mut radios, node, &facts(15, 1_000_000), || due);
        assert!(!radios[&(node, "radio1".to_owned())].idle(500));
        update(&mut radios, node, &facts(30, 1_000_100), || due);
        assert!(
            !radios[&(node, "radio1".to_owned())].idle(500),
            "two samples"
        );
        update(&mut radios, node, &facts(45, 1_000_200), || due);
        assert!(radios[&(node, "radio1".to_owned())].idle(500));
        // The radio goes from the facts: it is forgotten.
        let mut gone = facts(60, 0);
        gone.radios.clear();
        update(&mut radios, node, &gone, || due);
        assert_eq!(radios.len(), 0);
    }
}
