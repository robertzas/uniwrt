//! Transmit power (features/wifi.md, RF planner: Power): each radio's power set so the loudest
//! other node hears it at −65 to −72 dBm (enough overlap to roam, not so much that cells
//! drown each other), held up for its mesh peers, 2.4 GHz kept under 5 GHz, never under a
//! floor.

use uniwrt_model::{site::wireless::TxPower, wifi::Band};

use super::{
    input::{Link, PlanRadio},
    output::Reason,
};

/// The loudest a neighbor should hear a radio, and the quietest.
const LOUD_DBM: i16 = -65;
const QUIET_DBM: i16 = -72;
/// A mesh peer still hears its link at this, well above usual peer thresholds (−80 dBm).
const MESH_DBM: i16 = -70;
/// The least power planned.
const FLOOR_DBM: i16 = 10;
/// Smaller steps aren't worth a change.
const MIN_STEP_DB: i16 = 2;
/// How far 2.4 GHz stays under its node's 5 GHz radio.
const UNDER_5G_DB: i16 = 3;

/// A radio's planned power: the dBm, or `None` to leave it to the radio.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Power {
    pub dbm: Option<i16>,
    pub reasons: Vec<Reason>,
}

impl Power {
    /// How much louder (or quieter, below 0) the radio will be.
    pub(crate) fn shift(&self, radio: &PlanRadio) -> i16 {
        match (self.dbm, radio.current.and_then(|c| c.tx_power_dbm)) {
            (Some(planned), Some(now)) => planned - now,
            _ => 0,
        }
    }
}

/// Plans every radio's power; `radios` and the result are in the same order.
pub(crate) fn plan(radios: &[PlanRadio], links: &[Link]) -> Vec<Power> {
    let mut out: Vec<Power> = radios.iter().map(|r| one(r, links)).collect();
    for (i, radio) in radios.iter().enumerate() {
        if radio.band != Band::G2 || radio.tx_power != TxPower::Auto {
            continue;
        }
        let five = radios
            .iter()
            .zip(&out)
            .filter(|(r, _)| r.node == radio.node && r.band == Band::G5)
            .filter_map(|(r, p)| p.dbm.or(r.current.and_then(|c| c.tx_power_dbm)))
            .max();
        let (Some(five), Some(now)) = (five, out[i].dbm) else {
            continue;
        };
        let cap = (five - UNDER_5G_DB).max(FLOOR_DBM);
        if now > cap {
            out[i].dbm = Some(cap);
            out[i].reasons.push(Reason::BelowFiveGhz);
        }
    }
    out
}

fn one(radio: &PlanRadio, links: &[Link]) -> Power {
    let now = radio.current.and_then(|c| c.tx_power_dbm);
    match radio.tx_power {
        TxPower::Dbm(dbm) => {
            return Power {
                dbm: Some(i16::from(dbm)),
                reasons: vec![Reason::PinnedPower],
            };
        }
        TxPower::Auto => {}
    }
    let Some(now) = now else {
        return Power {
            dbm: None,
            reasons: Vec::new(),
        };
    };
    let max = radio
        .channels
        .iter()
        .filter_map(|c| c.max_tx_power_dbm)
        .max()
        .unwrap_or(now)
        .max(now);
    let hearing = || {
        links
            .iter()
            .filter(|l| l.band == radio.band && l.to == radio.node && l.from != radio.node)
    };
    let mut reasons = Vec::new();
    let mut target = now;
    if let Some(loudest) = hearing().map(|l| l.signal_dbm).max() {
        if loudest > LOUD_DBM {
            target = now - (loudest - LOUD_DBM);
            reasons.push(Reason::PowerLowered { heard_dbm: loudest });
        } else if loudest < QUIET_DBM {
            target = now + (QUIET_DBM - loudest);
            reasons.push(Reason::PowerRaised { heard_dbm: loudest });
        }
    }
    // The weakest mesh peer sets how low it may go.
    if let Some(peer) = hearing()
        .filter(|l| l.mesh)
        .min_by_key(|l| (l.signal_dbm, l.from))
    {
        let least = now - (peer.signal_dbm - MESH_DBM);
        if target < least {
            target = least;
            reasons.retain(|r| !matches!(r, Reason::PowerLowered { .. }));
            reasons.push(Reason::MeshPower {
                peer: peer.from,
                signal_dbm: peer.signal_dbm,
            });
        }
    }
    let target = target.clamp(FLOOR_DBM.min(max), max);
    if (target - now).abs() < MIN_STEP_DB {
        reasons.clear();
        return Power {
            dbm: Some(now),
            reasons,
        };
    }
    Power {
        dbm: Some(target),
        reasons,
    }
}
