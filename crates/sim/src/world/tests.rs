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
            &std::collections::BTreeMap::default(),
            &std::collections::BTreeSet::default(),
            render::wpad::MESH
        ),
        render::collect(
            &b,
            2,
            &uniwrt_node::proto::ops::Set::ALL,
            &std::collections::BTreeMap::default(),
            &std::collections::BTreeSet::default(),
            render::wpad::MESH
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

#[test]
fn steering_nodes_move_their_802_11v_clients() {
    let (mut walks, mut bands) = (0, 0);
    for seed in 0..6 {
        let config = SimConfig {
            seed,
            reboot_every: None,
            ..SimConfig::default()
        };
        let mut world = World::new(&config, START);
        for node in &mut world.nodes {
            node.steers = true;
        }
        let happenings = run(&mut world, 4 * 3600);
        for (i, happening) in happenings.iter().enumerate() {
            let What::Steered { mac, band, target } = happening.what else {
                continue;
            };
            let spec = world.clients.iter().find(|c| c.mac == mac).unwrap().spec;
            assert!(spec.ft || spec.sae, "only 802.11v clients are steered");
            // The client goes where it agreed to: the target BSS authorizes it next.
            let joined = happenings[i..]
                .iter()
                .find_map(|h| match h.what {
                    What::Authorized { mac: m, band, .. } if m == mac => Some((h.node, band)),
                    _ => None,
                })
                .expect("the steered client joins again");
            assert_eq!(world.nodes[joined.0].bssid(joined.1), target);
            if joined.0 == happening.node {
                assert_eq!((band, joined.1), (Band::G2, Band::G5), "band steering");
                bands += 1;
            } else {
                walks += 1;
            }
        }
    }
    assert!(
        walks > 0 && bands > 0,
        "{walks} steered roams, {bands} band steerings"
    );
}

#[test]
fn nodes_without_usteer_steer_nothing() {
    let mut world = World::new(&SimConfig::default(), START);
    let happenings = run(&mut world, 3600);
    assert!(
        !happenings
            .iter()
            .any(|h| matches!(h.what, What::Steered { .. }))
    );
}
