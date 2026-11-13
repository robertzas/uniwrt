//! The planner over simulated sites (`world`): the constraints hold, the plans make sense for
//! the air they're planned in, and the search is deterministic and finds the optimum.

use uniwrt_core::NodeId;
use uniwrt_model::{
    facts::DfsState,
    rf::DfsPolicy,
    site::wireless::TxPower,
    wifi::{Band, ChannelWidth},
};

use super::{
    BandPlan, ChannelStat, Plan, RadioPlan, Reason, Search,
    blocks::WEATHER_MHZ,
    search::{self, Space},
    world::{Foreign, World, channels, mhz, radio},
};
use crate::span::Span;

fn band(plan: &Plan, band: Band) -> &BandPlan {
    plan.bands
        .iter()
        .find(|b| b.band == band)
        .expect("the band")
}

fn of(plan: &BandPlan, node: i64) -> &RadioPlan {
    plan.radios
        .iter()
        .find(|r| r.node == NodeId::new(node))
        .expect("the node's radio")
}

fn span(r: &RadioPlan) -> Span {
    Span::new(r.planned.center_frequency_mhz, r.planned.width)
}

fn overlapping(plan: &BandPlan) -> usize {
    let mut n = 0;
    for (i, a) in plan.radios.iter().enumerate() {
        for b in &plan.radios[i + 1..] {
            if span(a).overlap(span(b)) > 0.0 {
                n += 1;
            }
        }
    }
    n
}

/// `n` nodes `spacing` meters apart on a line, each with one radio of `band` on `primary`.
fn row(n: i64, spacing: f32, band: Band, primary: u16, width: ChannelWidth) -> World {
    let mut w = World::default();
    for id in 1..=n {
        #[allow(clippy::cast_precision_loss)]
        let x = (id - 1) as f32 * spacing;
        w.node(id, (x, 0.0), vec![radio(id, band, primary, width, 20)]);
    }
    w
}

#[test]
fn two_ghz_takes_one_non_overlapping_set() {
    let w = row(3, 8.0, Band::G2, 6, mhz(20));
    let plan = w.plan();
    let g2 = band(&plan, Band::G2);
    let mut got: Vec<u16> = g2.radios.iter().map(|r| r.planned.channel).collect();
    got.sort_unstable();
    assert_eq!(got, [1, 6, 11]);
    assert!(g2.improvement > 0.5, "{g2:?}");
    assert_eq!(g2.search, Search::Exhaustive);
    assert!(g2.optimal);

    // Where every radio allows 13, four nodes take 1/5/9/13.
    let mut w = row(4, 8.0, Band::G2, 6, mhz(20));
    for n in &mut w.nodes {
        n.radios[0].channels = channels(Band::G2, 13);
    }
    let plan = w.plan();
    let mut got: Vec<u16> = band(&plan, Band::G2)
        .radios
        .iter()
        .map(|r| r.planned.channel)
        .collect();
    got.sort_unstable();
    assert_eq!(got, [1, 5, 9, 13]);
}

#[test]
fn mesh_radios_share_one_channel_and_width() {
    let mut w = row(3, 10.0, Band::G5, 36, mhz(80));
    for n in &mut w.nodes {
        n.radios[0].mesh = true;
    }
    w.nodes[2].radios[0].widths.remove(&mhz(160));
    w.foreign.push(Foreign {
        at: (10.0, 3.0),
        band: Band::G5,
        primary: 36,
        width: mhz(80),
        tx_dbm: 20.0,
    });
    let plan = w.plan();
    let g5 = band(&plan, Band::G5);
    let first = g5.radios[0].planned;
    for r in &g5.radios {
        assert_eq!(
            (r.planned.channel, r.planned.width),
            (first.channel, first.width)
        );
        assert!(r.reasons.contains(&Reason::MeshGroup { radios: 3 }));
    }
    assert!(first.width <= mhz(80), "one radio can't do 160");
    assert!(
        !span(&g5.radios[0]).contains(5210),
        "away from the foreign network"
    );
}

#[test]
fn pins_hold_and_a_mesh_follows_the_first() {
    let mut w = row(3, 10.0, Band::G5, 36, mhz(80));
    w.nodes[0].radios[0].pin = Some(149);
    w.nodes[1].radios[0].pin_width = Some(mhz(20));
    w.nodes[2].radios[0].tx_power = TxPower::Dbm(14);
    let plan = w.plan();
    let g5 = band(&plan, Band::G5);
    assert_eq!(of(g5, 1).planned.channel, 149);
    assert!(of(g5, 1).reasons.contains(&Reason::PinnedChannel));
    assert_eq!(of(g5, 2).planned.width, mhz(20));
    assert!(of(g5, 2).reasons.contains(&Reason::PinnedWidth));
    assert_eq!(of(g5, 3).planned.tx_power_dbm, Some(14));
    assert!(of(g5, 3).reasons.contains(&Reason::PinnedPower));

    for n in &mut w.nodes {
        n.radios[0].mesh = true;
        n.radios[0].pin_width = None;
    }
    w.nodes[2].radios[0].pin = Some(36);
    let plan = w.plan();
    let g5 = band(&plan, Band::G5);
    assert!(g5.radios.iter().all(|r| r.planned.channel == 149));
    assert!(of(g5, 3).reasons.contains(&Reason::PinConflict {
        node: NodeId::new(1),
        radio: "radio1".to_owned(),
    }));
}

#[test]
fn the_dfs_policy_limits_the_channels() {
    let crowded = |dfs| {
        let mut w = row(6, 6.0, Band::G5, 36, mhz(80));
        w.dfs = dfs;
        band(&w.plan(), Band::G5).clone()
    };
    let dfs_of = |r: &RadioPlan| {
        channels(Band::G5, 11)
            .into_iter()
            .filter(|c| span(r).contains(c.frequency_mhz) && c.frequency_mhz != span(r).low)
            .filter(|c| c.frequency_mhz != span(r).high)
            .collect::<Vec<_>>()
    };
    let avoid = crowded(DfsPolicy::Avoid);
    assert!(avoid.radios.iter().flat_map(dfs_of).all(|c| !c.dfs));
    let non_weather = crowded(DfsPolicy::NonWeather);
    assert!(
        non_weather
            .radios
            .iter()
            .flat_map(dfs_of)
            .all(|c| !WEATHER_MHZ.contains(&c.frequency_mhz))
    );
    let all = crowded(DfsPolicy::All);
    assert!(all.radios.iter().flat_map(dfs_of).any(|c| c.dfs));
    assert!(overlapping(&all) <= overlapping(&avoid));

    // Radar's non-occupancy keeps a radio off, whatever the policy.
    let mut w = row(1, 0.0, Band::G5, 36, mhz(20));
    w.foreign.push(Foreign {
        at: (1.0, 0.0),
        band: Band::G5,
        primary: 36,
        width: mhz(160),
        tx_dbm: 20.0,
    });
    for c in &mut w.nodes[0].radios[0].channels {
        if c.dfs {
            c.dfs_state = Some(DfsState::Unavailable);
        }
    }
    let plan = w.plan();
    let r = &band(&plan, Band::G5).radios[0];
    assert!(r.planned.channel >= 149, "{r:?}");
}

#[test]
fn radar_history_steers_away() {
    // Two radios where 36–48 and 149–161 are loud: they move onto DFS, but not where radar hit.
    let mut w = row(2, 5.0, Band::G5, 36, mhz(80));
    for primary in [36, 149] {
        w.foreign.push(Foreign {
            at: (2.0, 1.0),
            band: Band::G5,
            primary,
            width: mhz(80),
            tx_dbm: 20.0,
        });
    }
    let plan = w.plan();
    let first: Vec<u32> = band(&plan, Band::G5)
        .radios
        .iter()
        .map(|r| r.planned.center_frequency_mhz)
        .collect();
    for channel in [52, 56, 60, 64, 100, 104, 108, 112, 116, 120, 124, 128] {
        w.stats.push(ChannelStat {
            band: Band::G5,
            channel,
            busy_percent: None,
            radar_hits: 3,
        });
    }
    let plan = w.plan();
    let g5 = band(&plan, Band::G5);
    for r in &g5.radios {
        assert!(r.planned.channel >= 132, "{r:?} (before radar: {first:?})");
        assert!(r.after.dfs < 1.0, "{r:?}");
    }
}

#[test]
fn widths_widen_in_clean_air_and_narrow_when_crowded() {
    let w = row(1, 0.0, Band::G5, 36, mhz(80));
    let plan = w.plan();
    let r = &band(&plan, Band::G5).radios[0];
    assert_eq!(r.planned.width, mhz(160), "{r:?}");
    let mut w = row(1, 0.0, Band::G6, 37, mhz(160));
    w.nodes[0].radios[0].current = None;
    let plan = w.plan();
    let r = &band(&plan, Band::G6).radios[0];
    assert_eq!(r.planned.width, mhz(320));

    // Five close nodes and no DFS: two 80 MHz blocks can't hold them, so they narrow.
    let mut w = row(5, 4.0, Band::G5, 36, mhz(80));
    w.dfs = DfsPolicy::Avoid;
    let plan = w.plan();
    let g5 = band(&plan, Band::G5);
    assert_eq!(overlapping(g5), 0, "{g5:#?}");
    assert!(g5.radios.iter().any(|r| r.planned.width < mhz(80)));
    assert!(
        g5.radios
            .iter()
            .filter(|r| r.planned.width < mhz(160))
            .all(|r| r
                .reasons
                .iter()
                .any(|x| matches!(x, Reason::Narrowed { .. })))
    );
}

#[test]
fn the_primary_avoids_a_foreign_primary() {
    let mut w = row(1, 0.0, Band::G5, 149, mhz(80));
    let r = &mut w.nodes[0].radios[0];
    r.channels.retain(|c| (149..=161).contains(&c.number));
    r.widths.remove(&mhz(160));
    w.foreign.push(Foreign {
        at: (3.0, 0.0),
        band: Band::G5,
        primary: 149,
        width: mhz(20),
        tx_dbm: 20.0,
    });
    let plan = w.plan();
    let r = &band(&plan, Band::G5).radios[0];
    assert_eq!(
        (r.planned.width, r.planned.center_frequency_mhz),
        (mhz(80), 5775)
    );
    assert_eq!(r.planned.channel, 153);
}

#[test]
fn power_aims_for_roaming_overlap() {
    // 3 m apart at 20 dBm: each hears the other at about −41 dBm, so both turn down to the floor.
    let mut w = row(2, 3.0, Band::G5, 36, mhz(80));
    let plan = w.plan();
    let r = of(band(&plan, Band::G5), 1);
    assert_eq!(r.planned.tx_power_dbm, Some(10));
    assert!(matches!(
        r.reasons.last(),
        Some(Reason::PowerLowered { .. })
    ));

    // Mesh peers 25 m apart hear each other at about −69 dBm: the mesh keeps the power.
    let mut far = row(2, 25.0, Band::G5, 36, mhz(80));
    for n in &mut far.nodes {
        n.radios[0].mesh = true;
    }
    let plan = far.plan();
    assert_eq!(of(band(&plan, Band::G5), 1).planned.tx_power_dbm, Some(20));

    // 2.4 GHz stays 3 dB under the node's 5 GHz radio.
    w = row(2, 30.0, Band::G5, 36, mhz(80));
    for n in &mut w.nodes {
        let id = n.id.get();
        n.radios.push(radio(id, Band::G2, 1, mhz(20), 20));
    }
    let plan = w.plan();
    let five = of(band(&plan, Band::G5), 1)
        .planned
        .tx_power_dbm
        .expect("power");
    let two = of(band(&plan, Band::G2), 1);
    assert!(
        two.planned.tx_power_dbm.expect("power") <= five - 3,
        "{two:?}"
    );
}

#[test]
fn radios_of_one_node_never_overlap() {
    let mut w = World::default();
    let mut high = radio(1, Band::G5, 36, mhz(80), 20);
    high.radio = "radio2".to_owned();
    w.node(
        1,
        (0.0, 0.0),
        vec![radio(1, Band::G5, 36, mhz(80), 20), high],
    );
    let plan = w.plan();
    assert_eq!(overlapping(band(&plan, Band::G5)), 0);
}

#[test]
fn a_plan_is_a_fixed_point_and_order_free() {
    let mut w = World::default();
    for id in 1..=5i64 {
        #[allow(clippy::cast_precision_loss)]
        let at = ((id % 3) as f32 * 9.0, (id / 3) as f32 * 11.0);
        w.node(
            id,
            at,
            vec![
                radio(id, Band::G2, 6, mhz(20), 20),
                radio(id, Band::G5, 36, mhz(80), 23),
            ],
        );
    }
    w.foreign.push(Foreign {
        at: (4.0, 20.0),
        band: Band::G5,
        primary: 100,
        width: mhz(80),
        tx_dbm: 23.0,
    });
    let plan = w.plan();
    let mut shuffled = w.radios();
    shuffled.reverse();
    assert_eq!(w.plan_of(&shuffled), plan, "order free");
    let again = w.plan_of(&w.applied(&plan));
    for b in &again.bands {
        assert!(b.radios.iter().all(|r| !r.changed), "{b:#?}");
        assert!(b.improvement.abs() < 0.01);
        let sum: f32 = b.radios.iter().map(|r| r.after.total).sum();
        assert!((sum - b.after).abs() < 0.05, "the breakdowns add up");
    }
}

#[test]
fn large_sites_anneal_and_improve() {
    let mut w = World::default();
    for id in 0..36i64 {
        #[allow(clippy::cast_precision_loss)]
        let at = ((id % 6) as f32 * 12.0, (id / 6) as f32 * 12.0);
        w.node(id + 1, at, vec![radio(id + 1, Band::G5, 36, mhz(80), 23)]);
    }
    let plan = w.plan();
    let g5 = band(&plan, Band::G5);
    assert_eq!(g5.search, Search::Annealing);
    assert!(!g5.optimal);
    assert!(g5.improvement > 0.5, "{} → {}", g5.before, g5.after);
    assert_eq!(w.plan(), plan, "deterministic");
}

/// Spans, unary costs and pair weights.
type Tables = (Vec<Vec<Span>>, Vec<Vec<f32>>, Vec<Vec<f32>>);

/// A random space of `units` units with `choices` choices each over 20 MHz channels.
fn space(seed: u64, units: usize, choices: usize) -> Tables {
    let mut x = seed;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        #[allow(clippy::cast_precision_loss)]
        let v = (x % 1000) as f32 / 1000.0;
        v
    };
    let spans = (0..units)
        .map(|_| {
            (0..choices)
                .map(|c| Span::channel(5180 + 20 * u32::try_from(c).unwrap_or(0)))
                .collect()
        })
        .collect();
    let unary = (0..units)
        .map(|_| (0..choices).map(|_| next()).collect())
        .collect();
    let pair = (0..units)
        .map(|_| (0..units).map(|_| next() * 2.0).collect())
        .collect();
    (spans, unary, pair)
}

#[test]
fn annealing_finds_the_exhaustive_optimum_on_small_spaces() {
    for seed in 1..=20 {
        let (spans, unary, pair) = space(seed * 7919, 6, 5);
        let s = Space {
            spans,
            unary: &unary,
            pair: &pair,
        };
        let best = search::exhaustive_or_none(&s, vec![0; 6]).expect("small enough");
        let annealed = search::annealed(&s, vec![0; 6]);
        assert!(
            s.total(&annealed) <= s.total(&best) * 1.02 + 1e-4,
            "seed {seed}: {} vs {}",
            s.total(&annealed),
            s.total(&best)
        );
    }
}
