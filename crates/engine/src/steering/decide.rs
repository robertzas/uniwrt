//! What steering asks of whom, from a node's facts and usteer's view of a client: pure.

use std::net::IpAddr;

use uniwrt_core::{MacAddr, NodeId};
use uniwrt_model::{
    facts::{NodeFacts, StationCapability, WifiFeature, WifiIfaceMode},
    site::steering::SteeringPolicy,
    wifi::Band,
};
use uniwrt_node::proto::ops::{Btm, HeardAt, OwnReport, candidate};

use super::OwnReports;

/// A client steering may ask to move: authorized and with 802.11v on an access point of a
/// steered SSID, its signal known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Station {
    pub(super) mac: MacAddr,
    pub(super) iface: String,
    pub(super) ssid: String,
    pub(super) band: Band,
    pub(super) signal_dbm: i16,
}

/// The node's clients steering may ask to move; none where its hostapd has no 802.11v.
pub(super) fn stations(facts: &NodeFacts, ssids: &[&str]) -> Vec<Station> {
    if !facts
        .capabilities
        .wifi
        .contains(&WifiFeature::BssTransition)
    {
        return Vec::new();
    }
    facts
        .stations
        .iter()
        .filter(|s| s.authorized && s.capabilities.contains(&StationCapability::BssTransition))
        .filter_map(|s| {
            let (band, ssid) = access_point(facts, &s.iface)?;
            let signal_dbm = s.signal_dbm?;
            ssids.contains(&ssid).then(|| Station {
                mac: s.mac,
                iface: s.iface.clone(),
                ssid: ssid.to_owned(),
                band,
                signal_dbm,
            })
        })
        .collect()
}

/// The band and SSID of the node's access point `iface`.
fn access_point<'a>(facts: &'a NodeFacts, iface: &str) -> Option<(Band, &'a str)> {
    facts.radios.iter().find_map(|radio| {
        radio
            .ifaces
            .iter()
            .find(|i| i.mode == WifiIfaceMode::Ap && i.name.as_deref() == Some(iface))
            .and_then(|i| Some((radio.band, i.ssid.as_deref()?)))
    })
}

/// Band steering's targets for a 2.4 GHz client of `ssid`: the node's 5 and 6 GHz access
/// points of that SSID that are up.
pub(super) fn upper_bands(facts: &NodeFacts, ssid: &str) -> Vec<String> {
    facts
        .radios
        .iter()
        .filter(|radio| matches!(radio.band, Band::G5 | Band::G6))
        .flat_map(|radio| &radio.ifaces)
        .filter(|i| i.mode == WifiIfaceMode::Ap && i.up && i.ssid.as_deref() == Some(ssid))
        .filter_map(|i| i.name.clone())
        .collect()
}

/// Where each node's access points are: its addresses (usteer names another node's access
/// point by the address its usteer talks from) and its access points' SSIDs.
#[derive(Debug, Clone)]
pub(super) struct Place {
    pub(super) node: NodeId,
    pub(super) addresses: Vec<IpAddr>,
    /// Interface and SSID.
    pub(super) aps: Vec<(String, String)>,
}

impl Place {
    pub(super) fn of(node: NodeId, facts: &NodeFacts) -> Self {
        let addresses = facts
            .interfaces
            .iter()
            .flat_map(|interface| {
                let v4 = interface.ipv4.iter().map(|net| IpAddr::V4(net.addr()));
                let v6 = interface.ipv6.iter().map(|net| IpAddr::V6(net.addr()));
                v4.chain(v6)
            })
            .collect();
        let aps = facts
            .radios
            .iter()
            .flat_map(|radio| &radio.ifaces)
            .filter(|i| i.mode == WifiIfaceMode::Ap && i.up)
            .filter_map(|i| Some((i.name.clone()?, i.ssid.clone()?)))
            .collect();
        Self {
            node,
            addresses,
            aps,
        }
    }
}

/// An access point a client is asked to: node and interface.
pub(super) type Target = (NodeId, String);

/// A roam suggestion's targets for `station` at `node`, best first: the access points of its
/// SSID usteer hears it at, other than its own, at least the margin stronger than where it is
/// and not below the trigger themselves.
pub(super) fn roam_targets(
    node: NodeId,
    station: &Station,
    heard: &[HeardAt],
    places: &[Place],
    policy: &SteeringPolicy,
) -> Vec<Target> {
    let Some(trigger) = policy.roam_below_dbm else {
        return Vec::new();
    };
    let floor = (station.signal_dbm + policy.margin_db).max(trigger);
    let mut found: Vec<(i16, Target)> = heard
        .iter()
        .filter(|h| h.signal_dbm >= floor)
        .filter_map(|h| {
            let place = match h.host {
                None => places.iter().find(|p| p.node == node)?,
                Some(host) => places.iter().find(|p| p.addresses.contains(&host))?,
            };
            let ours = place
                .aps
                .iter()
                .any(|(iface, ssid)| *iface == h.iface && *ssid == station.ssid);
            let here = place.node == node && h.iface == station.iface;
            (ours && !here).then(|| (h.signal_dbm, (place.node, h.iface.clone())))
        })
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    found.dedup_by(|a, b| a.1 == b.1);
    found.into_iter().map(|(_, target)| target).collect()
}

/// The candidates of a request, best first with falling preferences: each target's own
/// report where its node gave one (an access point without 802.11k has none; the client then
/// chooses for itself among those it knows).
pub(super) fn candidates(targets: &[Target], own: &OwnReports) -> Vec<String> {
    targets
        .iter()
        .filter_map(|(node, iface)| {
            own.get(node)?
                .iter()
                .find(|r: &&OwnReport| r.ifname == *iface)
        })
        .take(Btm::MAX_NEIGHBORS)
        .zip((0..=u8::MAX).rev())
        .filter_map(|(report, preference)| candidate(&report.nr, preference))
        .collect()
}
