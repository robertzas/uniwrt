//! The mesh: 802.11s peer links and batman-adv neighbors (over Wi-Fi a mesh link, over Ethernet
//! a wired one), batman-adv's best paths toward the gateway, and the clients its translation
//! tables place.

use uniwrt_core::MacAddr;
use uniwrt_model::{
    facts::{BatmanFacts, NodeFacts, PeerLinkState},
    node::NodeRole,
};

use crate::{
    Medium,
    clients::Directory,
    fleet::{Fleet, band_tag, device, is_wireless, wifi_iface},
    links::{Graph, Link, Sample, key},
    quality::{signal_quality, tq_quality},
    wifi::capacity_mbps,
};

/// The mesh's links, and the best paths among them.
pub(crate) fn link(graph: &mut Graph, fleet: &Fleet<'_>, directory: &Directory) {
    for (member, node, facts) in fleet.with_facts() {
        peer_links(graph, fleet, directory, member, facts);
        for mesh in &facts.batman {
            neighbors(graph, fleet, directory, member, facts, mesh);
            if node.role != NodeRole::Gateway {
                best_path(graph, fleet, member, facts, mesh);
            }
        }
    }
}

/// 802.11s: every established peer link, and the others to adopted nodes (a link that does not
/// come up is worth seeing).
fn peer_links(
    graph: &mut Graph,
    fleet: &Fleet<'_>,
    directory: &Directory,
    member: usize,
    facts: &NodeFacts,
) {
    for station in &facts.stations {
        let Some(state) = station.peer_link else {
            continue;
        };
        let up = state == PeerLinkState::Established;
        let owner = fleet.owner(station.mac);
        if owner == Some(member) || (!up && owner.is_none()) {
            continue;
        }
        let peer = vertex(graph, fleet, directory, station.mac);
        let mut link = Link::new(fleet.vertex(member), peer, Medium::Mesh);
        link.ports[0] = Some(station.iface.clone());
        if let Some((radio, _)) = wifi_iface(facts, &station.iface) {
            link.band = Some(radio.band);
            link.tag = Some(band_tag(radio.band).to_owned());
        }
        link.up = up;
        link.signal_dbm = station.signal_dbm;
        link.quality = station.signal_dbm.map(signal_quality);
        link.capacity_mbps = capacity_mbps(station);
        link.sample = Some(Sample {
            at: facts.collected_at,
            out_bytes: station.tx_bytes,
            in_bytes: station.rx_bytes,
        });
        graph.link(link);
    }
}

/// batman-adv's neighbors: a mesh link over a Wi-Fi hard interface (the same link as its
/// 802.11s peer link), a wired one over any other.
fn neighbors(
    graph: &mut Graph,
    fleet: &Fleet<'_>,
    directory: &Directory,
    member: usize,
    facts: &NodeFacts,
    mesh: &BatmanFacts,
) {
    for neighbor in &mesh.neighbors {
        if fleet.owner(neighbor.mac) == Some(member) {
            continue;
        }
        let peer = vertex(graph, fleet, directory, neighbor.mac);
        let (medium, band) = medium(facts, &neighbor.hard_if);
        let mut link = Link::new(fleet.vertex(member), peer, medium);
        link.ports[0] = Some(neighbor.hard_if.clone());
        link.band = band;
        link.tag = band.map(|band| band_tag(band).to_owned());
        // The route to the neighbor itself, over the link.
        let routes = mesh.originators.iter().filter(|route| {
            route.next_hop == neighbor.mac
                && route.hard_if == neighbor.hard_if
                && fleet.vertex_of(route.mac) == fleet.vertex_of(neighbor.mac)
        });
        let (tq, throughput) = routes.fold((None, None), |(tq, kbps), route| {
            (tq.max(route.tq), kbps.max(route.throughput_kbps))
        });
        link.tq = tq;
        link.quality = tq.map(tq_quality);
        link.capacity_mbps = neighbor
            .throughput_kbps
            .or(throughput)
            .map(|kbps| kbps / 1000);
        if medium == Medium::Ethernet
            && let Some(dev) = device(facts, &neighbor.hard_if)
        {
            link.capacity_mbps = dev.speed_mbps.or(link.capacity_mbps);
            link.sample = Some(Sample::port(facts.collected_at, &dev.counters));
        }
        graph.link(link);
    }
}

/// Marks the link batman-adv routes over toward a gateway: to the gateway node itself, else to
/// the batman-adv gateway it chose.
fn best_path(
    graph: &mut Graph,
    fleet: &Fleet<'_>,
    member: usize,
    facts: &NodeFacts,
    mesh: &BatmanFacts,
) {
    let to_gateway = mesh.originators.iter().find(|route| {
        route.best
            && fleet
                .owner(route.mac)
                .is_some_and(|owner| fleet.members[owner].role == NodeRole::Gateway)
    });
    let hop = to_gateway
        .map(|route| (route.next_hop, route.hard_if.as_str()))
        .or_else(|| {
            let gateway = mesh.gateways.iter().find(|gateway| gateway.best)?;
            Some((gateway.next_hop, gateway.hard_if.as_str()))
        });
    if let Some((next_hop, hard_if)) = hop {
        let (medium, band) = medium(facts, hard_if);
        let tag = band.map(band_tag);
        graph.prefer(key(
            &fleet.vertex(member),
            &fleet.vertex_of(next_hop),
            medium,
            tag,
        ));
    }
}

/// Places the clients the translation tables know and no Wi-Fi station list did: `wired` ones
/// (after the bridges' forwarding databases had their say), or Wi-Fi ones. Clients behind a
/// mesh node that is not adopted, or an adopted one without facts, are placed there.
pub(crate) fn place(graph: &mut Graph, fleet: &Fleet<'_>, directory: &Directory, wired: bool) {
    for (member, _, facts) in fleet.with_facts() {
        for mesh in &facts.batman {
            for client in mesh
                .local_clients
                .iter()
                .filter(|client| client.wifi != wired)
            {
                place_at(
                    graph,
                    fleet,
                    directory,
                    client.mac,
                    fleet.vertex(member),
                    client.wifi,
                );
            }
            for client in mesh
                .remote_clients
                .iter()
                .filter(|client| client.wifi != wired)
            {
                let behind_unknown = fleet
                    .owner(client.originator)
                    .is_none_or(|owner| fleet.members[owner].facts.is_none());
                if behind_unknown {
                    let at = vertex(graph, fleet, directory, client.originator);
                    place_at(graph, fleet, directory, client.mac, at, client.wifi);
                }
            }
        }
    }
}

fn place_at(
    graph: &mut Graph,
    fleet: &Fleet<'_>,
    directory: &Directory,
    mac: MacAddr,
    at: String,
    wifi: bool,
) {
    if fleet.owner(mac).is_some() || graph.is_placed(mac) || !crate::fleet::unicast(mac) {
        return;
    }
    let client = graph.client(mac, directory);
    let medium = if wifi { Medium::Wifi } else { Medium::Ethernet };
    graph.link(Link::new(at, client, medium));
}

/// The vertex of a mesh neighbor: its node's, else a peer's.
fn vertex(graph: &mut Graph, fleet: &Fleet<'_>, directory: &Directory, mac: MacAddr) -> String {
    if fleet.owner(mac).is_some() {
        fleet.vertex_of(mac)
    } else {
        graph.peer(mac, directory)
    }
}

/// A mesh link over a Wi-Fi hard interface, on its radio's band; a wired one over another.
fn medium(facts: &NodeFacts, hard_if: &str) -> (Medium, Option<uniwrt_model::wifi::Band>) {
    if is_wireless(facts, hard_if) {
        let band = wifi_iface(facts, hard_if).map(|(radio, _)| radio.band);
        (Medium::Mesh, band)
    } else {
        (Medium::Ethernet, None)
    }
}
