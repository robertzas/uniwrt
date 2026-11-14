//! The coverage model (features/wifi.md, Coverage & floor plans; D-118): per band, a log-distance
//! path loss fitted to the inter-node RSSI matrix between placed nodes. The browser evaluates it
//! over each floor's plan for the heatmap, the dead zones and where to add a node.
//!
//! The loss over `d` meters crossing `k` floors is `L(1 m) + 10·n·log10(d) + k·F`: `L(1 m)` is
//! free space at the band's middle (it takes the antennas' gains as 0 dBi, which `n` absorbs), the
//! exponent `n` is fitted by least squares to the same-floor measurements, then the floor loss `F`
//! to the residuals of the measurements between floors. A part without measurements keeps the
//! band's default. Walls drawn on the plans add their materials' fixed losses (M6.2c), taken off
//! each measurement before the fit. A measurement is a matrix entry between two nodes placed on scaled floors:
//! the transmitter's power less the signal heard.

mod geometry;
#[cfg(test)]
mod tests;
mod walls;

use serde::{Deserialize, Serialize};
use uniwrt_core::NodeId;
use uniwrt_model::{
    floor::{Floor, Material},
    wifi::Band,
};

use self::{
    geometry::{Located, locate},
    walls::{storeys, wall_loss},
};
use crate::RfReport;

/// The fitted exponent stays within this.
pub const EXPONENTS: std::ops::RangeInclusive<f64> = 1.6..=6.0;
/// The fitted loss per floor crossed stays within this, in dB.
pub const FLOOR_LOSSES_DB: std::ops::RangeInclusive<f64> = 0.0..=40.0;
/// The power assumed for a radio that doesn't report one, in dBm.
pub const DEFAULT_TX_DBM: i16 = 20;

/// The model of each band the site's radios use (60 GHz isn't modeled).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Coverage {
    /// In band order.
    pub bands: Vec<BandModel>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct BandModel {
    pub band: Band,
    /// The loss over the first meter (free space), in dB.
    pub loss_at_1m_db: f32,
    pub exponent: f32,
    /// Fitted to measurements, else the band's default.
    pub exponent_fitted: bool,
    /// The loss per floor crossed, in dB.
    pub floor_loss_db: f32,
    pub floor_loss_fitted: bool,
    /// The measurements fitted.
    pub samples: u32,
    /// The root mean square of the model's misses over them, in dB.
    pub error_db: Option<f32>,
    /// Each node's strongest radio on the band that is up, by node.
    pub transmitters: Vec<Transmitter>,
    /// What crossing a wall of each material takes, lightest first.
    pub wall_losses: Vec<WallLoss>,
}

/// What a signal on the band loses crossing a wall of the material once.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct WallLoss {
    pub material: Material,
    pub loss_db: f32,
}

/// What a node sends with on the band.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Transmitter {
    pub node: NodeId,
    pub tx_power_dbm: i16,
}

/// The band's free-space loss at 1 m, default exponent and default loss per floor.
fn defaults(band: Band) -> Option<(f64, f64, f64)> {
    match band {
        Band::G2 => Some((40.2, 3.0, 12.0)),
        Band::G5 => Some((47.3, 3.3, 16.0)),
        Band::G6 => Some((48.7, 3.5, 18.0)),
        Band::G60 => None,
    }
}

/// One measurement: the loss seen less the walls crossed, `10·log10(d)` and the floors crossed.
#[derive(Debug, Clone, Copy)]
struct Sample {
    loss: f64,
    x: f64,
    floors: u16,
}

/// The model of every band of `report`, fitted to its matrix where `floors` place both nodes.
pub fn coverage(floors: &[Floor], report: &RfReport) -> Coverage {
    let located = locate(floors);
    let storeys = storeys(floors);
    let bands = report
        .bands
        .iter()
        .filter_map(|band| {
            let (at_1m, exponent, floor_loss) = defaults(band.band)?;
            let mut transmitters: Vec<Transmitter> = Vec::new();
            for radio in band.radios.iter().filter(|r| r.up) {
                let tx = radio.tx_power_dbm.unwrap_or(DEFAULT_TX_DBM);
                match transmitters.iter_mut().find(|t| t.node == radio.node) {
                    Some(t) => t.tx_power_dbm = t.tx_power_dbm.max(tx),
                    None => transmitters.push(Transmitter {
                        node: radio.node,
                        tx_power_dbm: tx,
                    }),
                }
            }
            transmitters.sort_by_key(|t| t.node);
            let at = |node: NodeId| located.iter().find(|l| l.node == node);
            let samples: Vec<Sample> = band
                .matrix
                .iter()
                .filter_map(|entry| {
                    let (from, to): (&Located, &Located) = (at(entry.from)?, at(entry.to)?);
                    let tx = transmitters.iter().find(|t| t.node == entry.to)?;
                    Some(Sample {
                        loss: f64::from(tx.tx_power_dbm)
                            - f64::from(entry.signal_dbm)
                            - wall_loss(&storeys, from, to, band.band),
                        x: 10.0 * from.distance(to).max(1.0).log10(),
                        floors: from.floors(to),
                    })
                })
                .collect();
            Some(fit(
                band.band,
                at_1m,
                (exponent, floor_loss),
                &samples,
                transmitters,
            ))
        })
        .collect();
    Coverage { bands }
}

fn fit(
    band: Band,
    at_1m: f64,
    (default_exponent, default_floor): (f64, f64),
    samples: &[Sample],
    transmitters: Vec<Transmitter>,
) -> BandModel {
    let (same, across): (Vec<&Sample>, Vec<&Sample>) = samples.iter().partition(|s| s.floors == 0);
    // Least squares through `L(1 m)`: n = Σ(L − L1)·x / Σx².
    let xx: f64 = same.iter().map(|s| s.x * s.x).sum();
    let exponent = (xx > 1e-6).then(|| {
        let xy: f64 = same.iter().map(|s| (s.loss - at_1m) * s.x).sum();
        (xy / xx).clamp(*EXPONENTS.start(), *EXPONENTS.end())
    });
    let n = exponent.unwrap_or(default_exponent);
    let floor_loss = (!across.is_empty()).then(|| {
        let per_floor: f64 = across
            .iter()
            .map(|s| (s.loss - at_1m - n * s.x) / f64::from(s.floors))
            .sum();
        #[allow(clippy::cast_precision_loss)]
        let mean = per_floor / across.len() as f64;
        mean.clamp(*FLOOR_LOSSES_DB.start(), *FLOOR_LOSSES_DB.end())
    });
    let f = floor_loss.unwrap_or(default_floor);
    let error_db = (!samples.is_empty()).then(|| {
        let squares: f64 = samples
            .iter()
            .map(|s| (s.loss - (at_1m + n * s.x + f * f64::from(s.floors))).powi(2))
            .sum();
        #[allow(clippy::cast_precision_loss)]
        let rms = (squares / samples.len() as f64).sqrt();
        round(rms)
    });
    BandModel {
        band,
        loss_at_1m_db: round(at_1m),
        exponent: round(n),
        exponent_fitted: exponent.is_some(),
        floor_loss_db: round(f),
        floor_loss_fitted: floor_loss.is_some(),
        samples: u32::try_from(samples.len()).unwrap_or(u32::MAX),
        error_db,
        transmitters,
        wall_losses: Material::ALL
            .iter()
            .map(|&material| WallLoss {
                material,
                #[allow(clippy::cast_possible_truncation)]
                loss_db: material.loss_db(band) as f32,
            })
            .collect(),
    }
}

/// Two decimals, so that models compare and serialize steadily.
#[allow(clippy::cast_possible_truncation)]
fn round(value: f64) -> f32 {
    ((value * 100.0).round() / 100.0) as f32
}
