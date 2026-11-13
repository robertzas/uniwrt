//! The world's dynamics over simulated hours: determinism, roaming, leaving, reboots.

use std::{collections::BTreeMap, time::Duration};

use uniwrt_core::{MacAddr, Timestamp};

use super::{Band, Happening, What, World};
use crate::{SimConfig, render};

const START: Timestamp = Timestamp::from_unix_ms(1_793_245_539_000);

fn run(world: &mut World, seconds: u64) -> Vec<Happening> {
    (0..seconds).flat_map(|_| world.step()).collect()
}

#[test]
fn a_seed_tells_one_story() {
    let config = SimConfig::default();
    let (mut a, mut b) = (World::new(&config, START), World::new(&config, START));
    assert_eq!(run(&mut a, 600), run(&mut b, 600));
    assert_eq!(
        render::collect(
            &a,
            2,
            &uniwrt_node::proto::ops::Set::ALL,
            &render::commands::Disk::default()
        ),
        render::collect(
            &b,
            2,
            &uniwrt_node::proto::ops::Set::ALL,
            &render::commands::Disk::default()
        )
    );
    let other = World::new(&SimConfig { seed: 1, ..config }, START);
    assert_ne!(
        a.clients.iter().map(|c| c.mac).collect::<Vec<_>>(),
        other.clients.iter().map(|c| c.mac).collect::<Vec<_>>()
    );
}

#[test]
fn the_house_is_dealt_as_configured() {
    let world = World::new(&SimConfig::default(), START);
    assert_eq!(world.now(), START);
    assert_eq!(world.nodes.len(), 4);
    assert_eq!(world.clients.len(), 30);
    let wired = world.clients.iter().filter(|c| c.port.is_some()).count();
    assert_eq!(wired, 2);
    // Addresses are unique, and randomized ones are locally administered.
    let mut macs: Vec<_> = world.clients.iter().map(|c| c.mac).collect();
    macs.extend(world.nodes.iter().map(|n| n.mac));
    macs.sort();
    macs.dedup();
    assert_eq!(macs.len(), 34);
    for client in &world.clients {
        assert!(!client.mac.is_multicast());
        assert_eq!(
            client.mac.is_locally_administered(),
            client.spec.oui.is_none()
        );
    }
    let big = World::new(
        &SimConfig {
            access_points: 12,
            clients: 400,
            ..SimConfig::default()
        },
        START,
    );
    let mut ips: Vec<_> = big.clients.iter().map(|c| c.ip).collect();
    ips.extend(big.nodes.iter().map(|n| n.ip));
    ips.sort();
    ips.dedup();
    assert_eq!(ips.len(), 413);
}

#[test]
fn clients_roam_leave_and_return() {
    let mut world = World::new(
        &SimConfig {
            reboot_every: None,
            ..SimConfig::default()
        },
        START,
    );
    let happenings = run(&mut world, 4 * 3600);
    // Where each client was last authorized, to count moves between nodes.
    let mut at: BTreeMap<MacAddr, usize> = BTreeMap::new();
    let (mut roams, mut fast, mut leaves) = (0, 0, 0);
    for happening in &happenings {
        match &happening.what {
            What::Authorized { mac, alg, .. } => {
                if at
                    .insert(*mac, happening.node)
                    .is_some_and(|before| before != happening.node)
                {
                    roams += 1;
                    fast += usize::from(*alg == "ft");
                }
            }
            What::Disassoc { .. } => leaves += 1,
            What::Down | What::Up => panic!("no reboots were asked for"),
            _ => {}
        }
    }
    assert!(roams >= 10, "{roams} roams");
    assert!(fast >= 5, "{fast} of {roams} roams were fast transitions");
    assert!(leaves >= roams, "{leaves} disassociations");
    for client in &world.clients {
        if let Some(assoc) = &client.assoc {
            assert!((-90.0..-15.0).contains(&assoc.signal), "{}", assoc.signal);
        }
    }
}

#[test]
fn a_reboot_moves_the_clients_and_comes_back() {
    let mut world = World::new(
        &SimConfig {
            reboot_every: None,
            ..SimConfig::default()
        },
        START,
    );
    let on_living = || -> usize { 1 };
    let living = on_living();
    let before = world.stations(living).count();
    assert!(before > 0);
    let mut out = Vec::new();
    assert!(world.reboot(living, &mut out));
    assert!(!world.reboot(living, &mut out), "already down");
    assert!(!world.reboot(0, &mut out), "the gateway does not reboot");
    assert_eq!(out.len(), 1);
    let happenings = run(&mut world, 30);
    assert_eq!(world.stations(living).count(), 0);
    let moved = happenings
        .iter()
        .filter(|h| matches!(h.what, What::Authorized { .. }) && h.node != living)
        .count();
    assert!(moved >= before, "{moved} of {before} clients moved");
    let happenings = run(&mut world, 60);
    assert!(
        happenings
            .iter()
            .any(|h| h.node == living && h.what == What::Up)
    );
    assert!(world.nodes[living].up());
    assert!(world.now_ms() - world.nodes[living].booted_ms < 120_000);
    assert_eq!(world.nodes[living].logd.records[0].id, 1);
}

#[test]
fn reboots_happen_about_as_often_as_configured() {
    let mut world = World::new(
        &SimConfig {
            reboot_every: Some(Duration::from_mins(30)),
            ..SimConfig::default()
        },
        START,
    );
    let downs = run(&mut world, 6 * 3600)
        .iter()
        .filter(|h| h.what == What::Down)
        .count();
    assert!((4..=24).contains(&downs), "{downs} reboots in 6 h");
}

#[test]
fn a_client_roams_soon_after_start() {
    // Demo mode and its tests always have a roam to show (the opening walk).
    for seed in (0..12).chain([42]) {
        let config = SimConfig {
            seed,
            reboot_every: None,
            ..SimConfig::default()
        };
        let mut world = World::new(&config, START);
        let mut at: BTreeMap<MacAddr, usize> = world
            .clients
            .iter()
            .filter_map(|c| Some((c.mac, c.assoc.as_ref()?.node)))
            .collect();
        // Between nodes demo mode adopts: all but the last.
        let last = world.nodes.len() - 1;
        let roamed = (1..=45).find(|_| {
            world.step().iter().any(|happening| match &happening.what {
                What::Authorized { mac, .. } => {
                    at.insert(*mac, happening.node).is_some_and(|before| {
                        before != happening.node && before != last && happening.node != last
                    })
                }
                _ => false,
            })
        });
        assert!(roamed.is_some(), "seed {seed}: no roam within 45 s");
    }
}

/// An 802.11v client connected somewhere, and the access points it could go to, best first.
fn steerable(world: &World) -> (usize, usize, Vec<MacAddr>) {
    let c = world
        .clients
        .iter()
        .position(|client| client.spec.sae && client.assoc.is_some())
        .expect("an 802.11v client connected");
    let at = world.clients[c].assoc.as_ref().unwrap();
    let (node, band) = (at.node, at.band);
    let mut others: Vec<(f64, MacAddr)> = (0..world.nodes.len())
        .flat_map(|n| Band::BOTH.map(|b| (n, b)))
        .filter(|&(n, b)| {
            (n, b) != (node, band) && (b == Band::G2 || world.clients[c].spec.five_ghz)
        })
        .map(|(n, b)| (world.client_signal(c, n, b), world.nodes[n].bssid(b)))
        .collect();
    others.sort_by(|a, b| b.0.total_cmp(&a.0));
    (
        c,
        node,
        others.into_iter().map(|(_, bssid)| bssid).collect(),
    )
}

#[test]
fn a_transition_request_moves_an_802_11v_client_where_asked() {
    let mut world = World::new(&SimConfig::default(), START);
    run(&mut world, 60);
    let (c, node, candidates) = steerable(&world);
    let mac = world.clients[c].mac;
    // Not connected there: not sent.
    let elsewhere = (node + 1) % world.nodes.len();
    assert!(world.transition(elsewhere, mac, &candidates).is_err());
    world.transition(node, mac, &candidates).unwrap();
    let happenings = run(&mut world, 5);
    let target = happenings
        .iter()
        .find_map(|h| match h.what {
            What::Steered { mac: m, target, .. } if m == mac && h.node == node => Some(target),
            _ => None,
        })
        .expect("the answer is pushed");
    assert_eq!(target, candidates[0]);
    // It goes where it agreed to: the target BSS authorizes it.
    let joined = happenings
        .iter()
        .find_map(|h| match h.what {
            What::Authorized { mac: m, band, .. } if m == mac => {
                Some(world.nodes[h.node].bssid(band))
            }
            _ => None,
        })
        .expect("the steered client joins again");
    assert_eq!(joined, target);
}

#[test]
fn a_client_without_802_11v_ignores_the_request() {
    let mut world = World::new(&SimConfig::default(), START);
    run(&mut world, 60);
    let c = world
        .clients
        .iter()
        .position(|client| !client.spec.sae && client.assoc.is_some())
        .expect("a client without 802.11v connected");
    let (mac, node) = (
        world.clients[c].mac,
        world.clients[c].assoc.as_ref().unwrap().node,
    );
    world.transition(node, mac, &[]).unwrap();
    assert!(
        !run(&mut world, 5)
            .iter()
            .any(|h| matches!(h.what, What::Steered { .. }))
    );
}

#[test]
fn usteer_tells_where_it_hears_a_client_and_steers_nothing_itself() {
    let mut world = World::new(&SimConfig::default(), START);
    let (c, node, _) = steerable(&world);
    let mac = world.clients[c].mac;
    // Where usteer doesn't run, nothing hears the client.
    assert_eq!(world.usteer_client_info(node, mac), None);
    for n in &mut world.nodes {
        n.steers = true;
    }
    let info = world.usteer_client_info(node, mac).unwrap();
    let nodes = info["nodes"].as_object().unwrap();
    let iface = world.clients[c].assoc.as_ref().unwrap().band.ap_iface();
    assert_eq!(nodes[&format!("hostapd.{iface}")]["connected"], true);
    let other = (node + 1) % world.nodes.len();
    let remote = format!("{}#hostapd.", world.nodes[other].ip);
    assert!(nodes.keys().any(|name| name.starts_with(&remote)), "{info}");
    assert!(nodes.values().all(|n| n["signal"].as_i64().unwrap() < 0));
    // The render turned usteer's own steering off: hours pass without a request.
    let happenings = run(&mut world, 4 * 3600);
    assert!(
        !happenings
            .iter()
            .any(|h| matches!(h.what, What::Steered { .. }))
    );
}
