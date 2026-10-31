//! Wi-Fi: each client at the access point it is associated with (the one it used last, while
//! it roams), and nodes that are another node's stations (a bridge's uplink).

use std::collections::BTreeMap;

use uniwrt_core::{MacAddr, Timestamp};
use uniwrt_model::facts::{RadioFacts, StationFacts, WifiIfaceFacts, WifiIfaceMode};

use crate::{
    Medium,
    clients::Directory,
    fleet::{Fleet, band_tag, wifi_iface},
    links::{Graph, Link, Sample},
    quality::signal_quality,
};

type Iface<'f> = Option<(&'f RadioFacts, &'f WifiIfaceFacts)>;

pub(crate) fn place(graph: &mut Graph, fleet: &Fleet<'_>, directory: &Directory) {
    let mut clients = BTreeMap::<MacAddr, (usize, Timestamp, &StationFacts, Iface<'_>)>::new();
    for (member, _, facts) in fleet.with_facts() {
        for station in facts
            .stations
            .iter()
            .filter(|station| station.peer_link.is_none())
        {
            let iface = wifi_iface(facts, &station.iface);
            let mode = iface.map(|(_, iface)| iface.mode);
            if matches!(
                mode,
                Some(WifiIfaceMode::Mesh | WifiIfaceMode::Adhoc | WifiIfaceMode::Monitor)
            ) {
                continue;
            }
            match fleet.owner(station.mac) {
                Some(owner) if owner == member => {}
                Some(_) => {
                    let peer = fleet.vertex_of(station.mac);
                    let link = link(
                        fleet.vertex(member),
                        peer,
                        station,
                        iface,
                        facts.collected_at,
                    );
                    graph.link(link);
                }
                // The access point a node's station interface is associated with, which is not
                // adopted, is not drawn.
                None if mode == Some(WifiIfaceMode::Sta) => {}
                None => {
                    let newer = clients.get(&station.mac).is_none_or(|(_, _, known, _)| {
                        (station.inactive_ms, station.connected_s)
                            < (known.inactive_ms, known.connected_s)
                    });
                    if newer {
                        clients.insert(station.mac, (member, facts.collected_at, station, iface));
                    }
                }
            }
        }
    }
    for (mac, (member, at, station, iface)) in clients {
        let client = graph.client(mac, directory);
        graph.link(link(fleet.vertex(member), client, station, iface, at));
    }
}

fn link(from: String, to: String, station: &StationFacts, iface: Iface<'_>, at: Timestamp) -> Link {
    let mut link = Link::new(from, to, Medium::Wifi);
    link.ports[0] = Some(station.iface.clone());
    if let Some((radio, iface)) = iface {
        link.band = Some(radio.band);
        link.tag = Some(band_tag(radio.band).to_owned());
        link.ssid.clone_from(&iface.ssid);
    }
    link.signal_dbm = station.signal_dbm;
    link.quality = station.signal_dbm.map(signal_quality);
    link.capacity_mbps = capacity_mbps(station);
    link.sample = Some(Sample {
        at,
        out_bytes: station.tx_bytes,
        in_bytes: station.rx_bytes,
    });
    link
}

/// The driver's expected throughput to the station, else the rate of the last frame sent to it.
pub(crate) fn capacity_mbps(station: &StationFacts) -> Option<u32> {
    station
        .expected_throughput_kbps
        .or(station.tx_rate.map(|rate| rate.kbps))
        .map(|kbps| kbps / 1000)
}
