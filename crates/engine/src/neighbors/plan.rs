//! Each BSS's neighbor list: pure, over the BSSes known and how the nodes hear each other.

use std::{cmp::Reverse, collections::BTreeMap};

use uniwrt_core::NodeId;
use uniwrt_model::wifi::Band;
use uniwrt_node::proto::ops::{NeighborReport, OwnReport};
use uniwrt_rf::RfReport;

/// One of the managed nodes' BSSes with 802.11k.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Bss {
    pub(crate) node: NodeId,
    /// From the node's facts; unknown while they don't list the interface.
    pub(crate) band: Option<Band>,
    pub(crate) report: OwnReport,
}

/// How strongly node `from` hears node `to`, by band (the RF report's matrix).
pub(crate) type Heard = BTreeMap<(Band, NodeId, NodeId), i16>;

/// The lists to send, by node, then interface.
pub(crate) type Lists = BTreeMap<NodeId, BTreeMap<String, Vec<NeighborReport>>>;

pub(crate) fn heard(report: &RfReport) -> Heard {
    report
        .bands
        .iter()
        .flat_map(|band| {
            band.matrix
                .iter()
                .map(|m| ((band.band, m.from, m.to), m.signal_dbm))
        })
        .collect()
}

/// Every BSS's neighbors: the other BSSes with its SSID, at most `max` of them, those of its
/// own node first (its other bands), then those of the nodes it hears best (on the neighbor's
/// band where the matrix has it, else on any). A BSS without any gets an empty list, which
/// clears a stale one.
pub(crate) fn lists(bsses: &[Bss], heard: &Heard, max: usize) -> Lists {
    let signal = |from: NodeId, to: NodeId, band: Option<Band>| {
        let on = |band: Band| {
            let there = heard.get(&(band, from, to));
            there.or_else(|| heard.get(&(band, to, from))).copied()
        };
        let any = || {
            heard
                .iter()
                .filter(|((_, f, t), _)| [*f, *t] == [from, to] || [*f, *t] == [to, from])
                .map(|(_, signal)| *signal)
                .max()
        };
        band.and_then(on).or_else(any)
    };
    let mut lists = Lists::new();
    for bss in bsses {
        let mut neighbors: Vec<&Bss> = bsses
            .iter()
            .filter(|n| n.report.ssid == bss.report.ssid && n.report.bssid != bss.report.bssid)
            .collect();
        neighbors.sort_by_key(|n| {
            let far = n.node != bss.node;
            let strength = Reverse(signal(bss.node, n.node, n.band));
            (far, strength, n.node, &n.report.bssid)
        });
        lists.entry(bss.node).or_default().insert(
            bss.report.ifname.clone(),
            neighbors
                .iter()
                .take(max)
                .map(|n| n.report.entry())
                .collect(),
        );
    }
    lists
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bss(node: i64, band: Band, ifname: &str, last: u8, ssid: &str) -> Bss {
        Bss {
            node: NodeId::new(node),
            band: Some(band),
            report: OwnReport {
                ifname: ifname.to_owned(),
                bssid: format!("02:00:00:00:0{node}:{last:02x}"),
                ssid: ssid.to_owned(),
                nr: format!("{last:02x}"),
            },
        }
    }

    fn named(list: &[NeighborReport]) -> Vec<&str> {
        list.iter().map(|n| n.0.as_str()).collect()
    }

    #[test]
    fn neighbors_share_the_ssid_with_the_node_itself_first_then_the_best_heard() {
        let bsses = [
            bss(1, Band::G2, "phy0-ap0", 0, "home"),
            bss(1, Band::G5, "phy1-ap0", 1, "home"),
            bss(1, Band::G5, "phy1-ap1", 2, "guest"),
            bss(2, Band::G5, "phy1-ap0", 1, "home"),
            bss(3, Band::G5, "phy1-ap0", 1, "home"),
            bss(3, Band::G2, "phy0-ap0", 0, "home"),
        ];
        let (n1, n2, n3) = (NodeId::new(1), NodeId::new(2), NodeId::new(3));
        // Node 1 hears node 3 better than node 2; on 2.4 GHz only the reverse direction is known.
        let heard = Heard::from([
            ((Band::G5, n1, n2), -70),
            ((Band::G5, n1, n3), -55),
            ((Band::G2, n3, n1), -40),
        ]);
        let lists = lists(&bsses, &heard, 16);
        assert_eq!(
            named(&lists[&n1]["phy0-ap0"]),
            [
                "02:00:00:00:01:01",
                "02:00:00:00:03:00",
                "02:00:00:00:03:01",
                "02:00:00:00:02:01"
            ]
        );
        assert_eq!(
            named(&lists[&n1]["phy1-ap0"]),
            [
                "02:00:00:00:01:00",
                "02:00:00:00:03:00",
                "02:00:00:00:03:01",
                "02:00:00:00:02:01"
            ]
        );
        // The guest network has no neighbors: its list is emptied.
        assert_eq!(lists[&n1]["phy1-ap1"], []);
        // Node 2 has no 2.4 GHz entry, so the 5 GHz one stands in; nothing says how it hears node 3.
        assert_eq!(
            named(&lists[&n2]["phy1-ap0"]),
            [
                "02:00:00:00:01:00",
                "02:00:00:00:01:01",
                "02:00:00:00:03:00",
                "02:00:00:00:03:01"
            ]
        );
        let entry = &lists[&n3]["phy0-ap0"][0];
        assert_eq!(
            entry,
            &NeighborReport("02:00:00:00:03:01".into(), "home".into(), "01".into())
        );
        // At most `max`.
        let few = super::lists(&bsses, &heard, 2);
        assert_eq!(few[&n1]["phy0-ap0"].len(), 2);
    }
}
