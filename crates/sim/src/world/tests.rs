//! The world's dynamics over simulated hours: determinism, roaming, leaving, reboots.

use std::{collections::BTreeMap, time::Duration};

use uniwrt_core::{MacAddr, Timestamp};

use super::{Happening, What, World};
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
        render::collect(&a, 2, &uniwrt_node::proto::ops::Set::ALL),
        render::collect(&b, 2, &uniwrt_node::proto::ops::Set::ALL)
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
