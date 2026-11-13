//! The nightly optimizer's decision (features/wifi.md, RF planner: Apply; M6.1 b3a, D-115) and
//! the site's document following a band's plan (D-112, as the web's `withBandPlan`).
//!
//! A band is worth applying when its plan improves its cost by [`NIGHTLY_IMPROVEMENT`] or more
//! and moves a radio the controller may change. Unapplied changes skip the night (the optimizer
//! never applies what nobody reviewed); a radio that would move and carries heavy traffic puts
//! the apply off until it quietens.

use std::collections::BTreeSet;

use uniwrt_core::NodeId;
use uniwrt_model::{
    rf::{BandGain, NIGHTLY_IMPROVEMENT},
    site::{
        SiteConfig,
        wireless::{PlannedChannel, PlannedRadio, TxPower},
    },
};

use super::{BandPlan, Plan, RadioPlan, RadioRef, Reason};

/// What the night does with a plan.
#[derive(Debug, Clone, PartialEq)]
pub enum Nightly {
    /// No band is worth applying.
    Nothing,
    /// Bands are worth applying, but the site has changes not yet applied: the night is skipped.
    PendingChanges(Vec<BandGain>),
    /// Bands are worth applying, but these radios that would move carry heavy traffic: try again
    /// later in the window.
    Traffic(Vec<BandGain>, Vec<RadioRef>),
    /// Apply these bands.
    Apply(Vec<BandGain>),
}

/// Whether the controller may change the radio (it isn't kept as it runs).
pub fn movable(radio: &RadioPlan) -> bool {
    !radio.reasons.contains(&Reason::Observed)
}

/// Whether the nightly optimizer applies the band's plan: it improves by
/// [`NIGHTLY_IMPROVEMENT`] or more, and it moves a radio the controller may change.
pub fn worth(band: &BandPlan) -> bool {
    band.improvement >= NIGHTLY_IMPROVEMENT && band.radios.iter().any(|r| r.changed && movable(r))
}

/// What the night does with `plan`, with `pending` changes or not, `busy` naming the radios
/// whose traffic is heavy.
pub fn decide(plan: &Plan, pending: bool, busy: &BTreeSet<(NodeId, String)>) -> Nightly {
    let bands: Vec<&BandPlan> = plan.bands.iter().filter(|b| worth(b)).collect();
    if bands.is_empty() {
        return Nightly::Nothing;
    }
    let gains = bands.iter().map(|b| gain(b)).collect();
    if pending {
        return Nightly::PendingChanges(gains);
    }
    let heavy: Vec<RadioRef> = bands
        .iter()
        .flat_map(|b| &b.radios)
        .filter(|r| r.changed && movable(r) && busy.contains(&(r.node, r.radio.clone())))
        .map(|r| RadioRef {
            node: r.node,
            radio: r.radio.clone(),
        })
        .collect();
    if heavy.is_empty() {
        Nightly::Apply(gains)
    } else {
        Nightly::Traffic(gains, heavy)
    }
}

/// The band's improvement in whole percent.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // 0 to 100 after the clamp
pub fn gain(band: &BandPlan) -> BandGain {
    BandGain {
        band: band.band,
        percent: (band.improvement.clamp(0.0, 1.0) * 100.0).round() as u8,
    }
}

/// `config` following the band's plan (D-112): the band's entries replaced by every radio the
/// controller may change (observed ones keep what they run, so they aren't stored), and the
/// mesh's channel and width from its group. Where the plan has no mesh radio, the mesh keeps
/// what was stored.
pub fn with_band_plan(config: &mut SiteConfig, band: &BandPlan) {
    let plan = &mut config.radio.plan;
    plan.radios.retain(|r| r.band != band.band);
    plan.radios.extend(
        band.radios
            .iter()
            .filter(|r| movable(r))
            .map(|r| PlannedRadio {
                node: r.node,
                radio: r.radio.clone(),
                band: band.band,
                channel: r.planned.channel,
                width: r.planned.width,
                tx_power: power(r.planned.tx_power_dbm),
            }),
    );
    plan.radios
        .sort_by(|a, b| (a.node, &a.radio).cmp(&(b.node, &b.radio)));
    let mesh = band.radios.iter().find(|r| {
        r.reasons
            .iter()
            .any(|reason| matches!(reason, Reason::MeshGroup { .. }))
    });
    if let Some(mesh) = mesh {
        plan.mesh = Some(PlannedChannel {
            band: band.band,
            channel: mesh.planned.channel,
            width: mesh.planned.width,
        });
    }
}

/// A planned power as the site stores it: the radio's maximum, or 1 to 30 dBm.
fn power(dbm: Option<i16>) -> TxPower {
    match dbm {
        Some(dbm) if dbm >= 1 => TxPower::Dbm(u8::try_from(dbm.min(30)).unwrap_or(30)),
        _ => TxPower::Auto,
    }
}

#[cfg(test)]
#[path = "nightly_tests.rs"]
mod tests;
