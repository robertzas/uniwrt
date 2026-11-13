//! The RF planner (features/wifi.md, RF planner; M6.1 a1, D-110): each radio's channel, width
//! and power, per band, from what the radios may do, how the nodes hear each other and the
//! foreign networks, the channels' history and the DFS policy.
//!
//! Power comes first ([`power`]); the channel search then weighs the radios' overlaps at the
//! planned powers. A band's mesh radios share one channel and width. Pins are hard constraints.
//! 2.4 GHz uses 20 MHz on one non-overlapping set (1/6/11, or 1/5/9/13 where every radio allows
//! 13: both are planned and the cheaper kept). Elsewhere widths are free up to the hardware
//! (D-110): the width given up is a cost, so the planner narrows only where the air is crowded.

pub(crate) mod blocks;
mod cost;
mod input;
mod output;
mod power;
mod search;
mod sources;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod world;

use std::collections::BTreeSet;

use uniwrt_model::wifi::Band;

use self::{
    cost::{Choice, Model},
    power::Power,
    search::Space,
};
pub use self::{
    input::{ChannelStat, Current, Heard, Link, PlanInputs, PlanRadio},
    output::{BandPlan, Breakdown, Plan, RadioPlan, RadioRef, Reason, Search, Setting},
    sources::{Gathered, Sources, gather},
};
use crate::span::Span;

/// Plans every band the radios use.
pub fn plan(inputs: &PlanInputs<'_>) -> Plan {
    let mut order: Vec<usize> = (0..inputs.radios.len()).collect();
    order.sort_by(|a, b| {
        let (a, b) = (&inputs.radios[*a], &inputs.radios[*b]);
        (a.band, a.node, &a.radio).cmp(&(b.band, b.node, &b.radio))
    });
    let power = power::plan(inputs.radios, inputs.links);
    let bands: BTreeSet<Band> = inputs.radios.iter().map(|r| r.band).collect();
    let bands = bands
        .into_iter()
        .filter_map(|band| {
            let of: Vec<usize> = order
                .iter()
                .copied()
                .filter(|i| inputs.radios[*i].band == band)
                .collect();
            let sets: Vec<Option<&[u16]>> = if band == Band::G2 {
                blocks::g2_sets(
                    &of.iter()
                        .map(|i| inputs.radios[*i].channels.as_slice())
                        .collect::<Vec<_>>(),
                )
                .into_iter()
                .map(Some)
                .collect()
            } else {
                vec![None]
            };
            sets.into_iter()
                .map(|set| band_plan(inputs, band, &of, &power, set))
                .min_by(|a, b| a.after.total_cmp(&b.after))
        })
        .collect();
    Plan { bands }
}

fn band_plan(
    inputs: &PlanInputs<'_>,
    band: Band,
    of: &[usize],
    power: &[Power],
    g2: Option<&[u16]>,
) -> BandPlan {
    let radios = of
        .iter()
        .map(|i| (&inputs.radios[*i], power[*i].shift(&inputs.radios[*i])))
        .collect();
    let model = Model::new(inputs, band, radios, g2);
    let (chosen, search, optimal) = choose(&model);
    let planned_spans: Vec<Option<Span>> = chosen.iter().map(|c| c.map(|c| c.span)).collect();
    let current_spans: Vec<Option<Span>> = model
        .radios
        .iter()
        .map(|r| r.current.map(|c| Choice::of(&c).span))
        .collect();
    let mut plans = Vec::new();
    let mut skipped = Vec::new();
    let (mut before, mut after) = (0.0, 0.0);
    for (i, radio) in model.radios.iter().enumerate() {
        let Some(choice) = chosen[i] else {
            skipped.push(RadioRef {
                node: radio.node,
                radio: radio.radio.clone(),
            });
            continue;
        };
        let power = &power[of[i]];
        let then = radio
            .current
            .map(|c| model.radio(i, Choice::of(&c), &current_spans, false));
        let now = model.radio(i, choice, &planned_spans, true);
        before += then.map_or(0.0, |b| b.total);
        after += now.total;
        let planned = Setting {
            channel: choice.primary,
            width: choice.width,
            center_frequency_mhz: u32::midpoint(choice.span.low, choice.span.high),
            tx_power_dbm: power.dbm,
        };
        let current = radio.current.map(|c| Setting {
            channel: c.channel,
            width: c.width,
            center_frequency_mhz: c.center_frequency_mhz,
            tx_power_dbm: c.tx_power_dbm,
        });
        plans.push(RadioPlan {
            node: radio.node,
            radio: radio.radio.clone(),
            changed: current.is_none_or(|c| {
                (c.channel, c.width, c.center_frequency_mhz)
                    != (planned.channel, planned.width, planned.center_frequency_mhz)
                    || planned
                        .tx_power_dbm
                        .is_some_and(|p| Some(p) != c.tx_power_dbm)
            }),
            current,
            planned,
            before: then.map(rounded),
            after: rounded(now),
            reasons: reasons(&model, i, choice, power),
        });
    }
    BandPlan {
        band,
        radios: plans,
        before: round(before),
        after: round(after),
        improvement: if before > 0.0 {
            round((before - after) / before)
        } else {
            0.0
        },
        search,
        optimal,
        skipped,
    }
}

/// Searches the band's units' choices: each radio's choice (`None`: skipped), how the search
/// ran and whether it is proven best.
fn choose(model: &Model<'_>) -> (Vec<Option<Choice>>, Search, bool) {
    let live: Vec<usize> = (0..model.units.len())
        .filter(|u| !model.units[*u].choices.is_empty())
        .collect();
    let (unary, pair) = model.tables();
    let unary: Vec<Vec<f32>> = live.iter().map(|u| unary[*u].clone()).collect();
    let pair: Vec<Vec<f32>> = live
        .iter()
        .map(|u| live.iter().map(|v| pair[*u][*v]).collect())
        .collect();
    let space = Space {
        spans: live
            .iter()
            .map(|u| model.units[*u].choices.iter().map(|c| c.span).collect())
            .collect(),
        unary: &unary,
        pair: &pair,
    };
    let start = live
        .iter()
        .zip(&unary)
        .map(|(u, costs)| start(model, *u, costs))
        .collect();
    let (assign, search, optimal) = search::solve(&space, start);

    let mut chosen: Vec<Option<Choice>> = vec![None; model.radios.len()];
    for (k, u) in live.iter().enumerate() {
        for i in &model.units[*u].radios {
            chosen[*i] = Some(model.units[*u].choices[assign[k]]);
        }
    }
    (chosen, search, optimal)
}

/// Where the search starts: what the unit runs, else its cheapest choice.
fn start(model: &Model<'_>, u: usize, costs: &[f32]) -> usize {
    let unit = &model.units[u];
    let current = unit
        .radios
        .iter()
        .find_map(|i| model.radios[*i].current)
        .map(|c| Choice::of(&c));
    let at = |same: &dyn Fn(&Choice) -> bool| unit.choices.iter().position(same);
    current
        .and_then(|c| at(&|x| *x == c).or_else(|| at(&|x| x.span == c.span)))
        .unwrap_or_else(|| {
            (0..costs.len())
                .min_by(|a, b| costs[*a].total_cmp(&costs[*b]).then(a.cmp(b)))
                .unwrap_or(0)
        })
}

fn reasons(model: &Model<'_>, i: usize, choice: Choice, power: &Power) -> Vec<Reason> {
    let radio = model.radios[i];
    let unit = &model.units[model.unit_of[i]];
    if radio.observed {
        // Pinned to what it runs: the pins are the controller's, not the site's.
        let mut out = vec![Reason::Observed];
        if radio.mesh {
            out.push(Reason::MeshGroup {
                radios: u16::try_from(unit.radios.len()).unwrap_or(u16::MAX),
            });
        }
        return out;
    }
    let mut out = Vec::new();
    match unit.pin {
        Some((by, pin)) if by != i && radio.pin.is_some_and(|p| p != pin) => {
            out.push(Reason::PinConflict {
                node: model.radios[by].node,
                radio: model.radios[by].radio.clone(),
            });
        }
        Some(_) if radio.pin.is_some() => out.push(Reason::PinnedChannel),
        _ => {}
    }
    if radio.pin_width.is_some() {
        out.push(Reason::PinnedWidth);
    }
    if radio.mesh {
        out.push(Reason::MeshGroup {
            radios: u16::try_from(unit.radios.len()).unwrap_or(u16::MAX),
        });
    }
    if unit.stuck {
        out.push(Reason::NoChannel);
    }
    if choice.width < unit.widest {
        out.push(Reason::Narrowed {
            widest: unit.widest,
        });
    }
    out.extend(power.reasons.iter().cloned());
    out
}

fn rounded(b: Breakdown) -> Breakdown {
    Breakdown {
        own: round(b.own),
        external: round(b.external),
        dfs: round(b.dfs),
        change: round(b.change),
        width: round(b.width),
        total: round(b.total),
    }
}

fn round(value: f32) -> f32 {
    (value * 100.0).round() / 100.0
}
