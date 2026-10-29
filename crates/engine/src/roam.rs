//! Inferring roams ([`Roam`]) across the fleet: where each Wi-Fi client is connected, from the
//! station events of every node and, for clients that joined before anyone watched, from the
//! nodes' station lists.

use std::{collections::HashMap, time::Duration};

use uniwrt_core::{MacAddr, NodeId, Timestamp};
use uniwrt_model::{
    event::{Attachment, Roam, StationChange, StationEvent},
    facts::StationFacts,
};

/// Where each client is (or was until shortly ago) connected.
#[derive(Debug)]
pub(crate) struct RoamTracker {
    /// How long after leaving an access point a client's join elsewhere is still a roam.
    window: Duration,
    clients: HashMap<MacAddr, Seen>,
    pruned_at: Timestamp,
}

#[derive(Debug)]
struct Seen {
    at: Attachment,
    /// When it left `at`; `None` while connected.
    left: Option<Timestamp>,
}

impl RoamTracker {
    pub(crate) fn new(window: Duration) -> Self {
        Self {
            window,
            clients: HashMap::new(),
            pruned_at: Timestamp::UNIX_EPOCH,
        }
    }

    /// Notes a station event of `node`; a client authorized at another access point than the
    /// one it is connected to, or left within the window, has roamed. A disassociation from the
    /// access point it roamed away from comes late, and changes nothing.
    pub(crate) fn observe(
        &mut self,
        node: NodeId,
        now: Timestamp,
        event: &StationEvent,
    ) -> Option<Roam> {
        self.prune(now);
        let here = Attachment {
            node,
            iface: event.iface.clone(),
        };
        match event.change {
            StationChange::Authorized => {
                let seen = Seen {
                    at: here.clone(),
                    left: None,
                };
                let before = self.clients.insert(event.mac, seen)?;
                let recent = before
                    .left
                    .is_none_or(|left| now.saturating_duration_since(left) <= self.window);
                (before.at != here && recent).then(|| Roam {
                    mac: event.mac,
                    from: before.at,
                    to: here,
                    auth_alg: event.auth_alg.clone(),
                })
            }
            StationChange::Disassociated => {
                if let Some(seen) = self.clients.get_mut(&event.mac)
                    && seen.at == here
                    && seen.left.is_none()
                {
                    seen.left = Some(now);
                }
                None
            }
            StationChange::AuthRequest | StationChange::AssocRequest => None,
        }
    }

    /// Notes the stations `node` reports: authorized clients not known yet are connected where
    /// they are listed, and clients connected to `node` but no longer listed have left (their
    /// disassociation was missed, such as while the node was down).
    pub(crate) fn seed(&mut self, node: NodeId, now: Timestamp, stations: &[StationFacts]) {
        for station in stations {
            if station.authorized && station.peer_link.is_none() {
                self.clients.entry(station.mac).or_insert_with(|| Seen {
                    at: Attachment {
                        node,
                        iface: station.iface.clone(),
                    },
                    left: None,
                });
            }
        }
        for (mac, seen) in &mut self.clients {
            if seen.at.node == node
                && seen.left.is_none()
                && !stations
                    .iter()
                    .any(|station| station.mac == *mac && station.iface == seen.at.iface)
            {
                seen.left = Some(now);
            }
        }
    }

    /// Forgets the clients of a node the engine no longer watches.
    pub(crate) fn forget(&mut self, node: NodeId) {
        self.clients.retain(|_, seen| seen.at.node != node);
    }

    /// Drops the clients that left longer than the window ago, at most once per window.
    fn prune(&mut self, now: Timestamp) {
        if now.saturating_duration_since(self.pruned_at) < self.window {
            return;
        }
        self.pruned_at = now;
        let window = self.window;
        self.clients.retain(|_, seen| {
            seen.left
                .is_none_or(|left| now.saturating_duration_since(left) <= window)
        });
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.clients.len()
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, time::Duration};

    use uniwrt_core::{MacAddr, NodeId, Timestamp};
    use uniwrt_model::{
        event::{Attachment, StationChange, StationEvent},
        facts::{PeerLinkState, StationFacts},
    };

    use super::RoamTracker;

    const MAC: &str = "02:00:00:00:04:00";

    fn event(iface: &str, change: StationChange, auth_alg: Option<&str>) -> StationEvent {
        StationEvent {
            mac: MAC.parse().unwrap(),
            iface: iface.to_owned(),
            change,
            signal_dbm: None,
            freq_mhz: None,
            auth_alg: auth_alg.map(str::to_owned),
        }
    }

    fn joined(iface: &str) -> StationEvent {
        event(iface, StationChange::Authorized, Some("sae"))
    }

    fn left(iface: &str) -> StationEvent {
        event(iface, StationChange::Disassociated, None)
    }

    fn at(node: i64, iface: &str) -> Attachment {
        Attachment {
            node: NodeId::new(node),
            iface: iface.to_owned(),
        }
    }

    fn secs(secs: i64) -> Timestamp {
        Timestamp::from_unix_secs(1_000 + secs)
    }

    fn tracker() -> RoamTracker {
        RoamTracker::new(Duration::from_secs(15))
    }

    #[test]
    fn a_client_joining_another_ap_while_connected_roams_and_the_late_leave_is_ignored() {
        let (a, b) = (NodeId::new(1), NodeId::new(2));
        let mut roams = tracker();
        assert_eq!(roams.observe(a, secs(0), &joined("phy1-ap0")), None);
        let ft = event("phy1-ap0", StationChange::Authorized, Some("ft"));
        let roam = roams.observe(b, secs(600), &ft).unwrap();
        assert_eq!(
            (roam.mac, roam.from, roam.to, roam.auth_alg.as_deref()),
            (
                MAC.parse::<MacAddr>().unwrap(),
                at(1, "phy1-ap0"),
                at(2, "phy1-ap0"),
                Some("ft")
            )
        );
        // The old AP times the client out later: it is still connected at b.
        assert_eq!(roams.observe(a, secs(900), &left("phy1-ap0")), None);
        let back = roams.observe(a, secs(901), &joined("phy1-ap0")).unwrap();
        assert_eq!((back.from, back.to), (at(2, "phy1-ap0"), at(1, "phy1-ap0")));
    }

    #[test]
    fn leaving_then_joining_elsewhere_roams_only_within_the_window() {
        let a = NodeId::new(1);
        let mut roams = tracker();
        roams.observe(a, secs(0), &joined("phy0-ap0"));
        roams.observe(a, secs(10), &left("phy0-ap0"));
        // Band steering on one node is a roam too.
        let roam = roams.observe(a, secs(20), &joined("phy1-ap0")).unwrap();
        assert_eq!((roam.from, roam.to), (at(1, "phy0-ap0"), at(1, "phy1-ap0")));
        roams.observe(a, secs(30), &left("phy1-ap0"));
        assert_eq!(roams.observe(a, secs(60), &joined("phy0-ap0")), None);
        // Reconnecting to the same AP is no roam.
        roams.observe(a, secs(70), &left("phy0-ap0"));
        assert_eq!(roams.observe(a, secs(71), &joined("phy0-ap0")), None);
        assert_eq!(
            roams.observe(
                a,
                secs(72),
                &event("phy0-ap0", StationChange::AssocRequest, None)
            ),
            None
        );
    }

    #[test]
    fn station_lists_seed_clients_and_end_the_ones_no_longer_listed() {
        let (a, b) = (NodeId::new(1), NodeId::new(2));
        let station = |iface: &str| StationFacts {
            mac: MAC.parse().unwrap(),
            iface: iface.to_owned(),
            signal_dbm: Some(-50),
            connected_s: 10,
            inactive_ms: 0,
            rx_rate: None,
            tx_rate: None,
            rx_bytes: 0,
            tx_bytes: 0,
            rx_packets: 0,
            tx_packets: 0,
            tx_retries: None,
            tx_failed: None,
            expected_throughput_kbps: None,
            rx_airtime_us: None,
            tx_airtime_us: None,
            peer_link: None,
            authorized: true,
            standard: None,
            capabilities: BTreeSet::new(),
        };
        let mut roams = tracker();
        roams.seed(a, secs(0), &[station("phy1-ap0")]);
        let roam = roams.observe(b, secs(5), &joined("phy1-ap0")).unwrap();
        assert_eq!(roam.from, at(1, "phy1-ap0"));

        let mut roams = tracker();
        roams.seed(a, secs(0), &[station("phy1-ap0")]);
        roams.seed(a, secs(100), &[]);
        assert_eq!(roams.observe(b, secs(200), &joined("phy1-ap0")), None);
        // Mesh peers are not clients.
        let mut roams = tracker();
        let peer = StationFacts {
            peer_link: Some(PeerLinkState::Established),
            ..station("mesh0")
        };
        roams.seed(a, secs(0), &[peer]);
        assert_eq!(roams.len(), 0);
    }

    #[test]
    fn clients_that_left_are_pruned_and_forgotten_with_their_node() {
        let (a, b) = (NodeId::new(1), NodeId::new(2));
        let mut roams = tracker();
        roams.observe(a, secs(0), &joined("phy1-ap0"));
        roams.observe(a, secs(1), &left("phy1-ap0"));
        let other = StationEvent {
            mac: "02:00:00:00:05:00".parse().unwrap(),
            ..joined("phy0-ap0")
        };
        roams.observe(b, secs(100), &other);
        assert_eq!(roams.len(), 1);
        roams.forget(b);
        assert_eq!(roams.len(), 0);
    }
}
