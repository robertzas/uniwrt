//! The experience score v1 (features/observability.md, Clients): 0–100, a weighted mean of
//! what a Wi-Fi client's connection is like, with each part kept so the UI can show why.
//!
//! | Factor | Weight | 0 at | 100 at |
//! |---|---|---|---|
//! | SNR (else signal) | 35 | 10 dB (−85 dBm) | 40 dB (−55 dBm) |
//! | PHY rate against what the client and radio can reach, while it moves traffic | 25 | 5 % | 50 % |
//! | Retries per frame sent (failures count fivefold) | 20 | 40 % | 5 % |
//! | Channel busy | 10 | 90 % | 40 % |
//! | Roams in the last hour (flapping) | 10 | 12 | 4 |
//!
//! A part without data drops out and the others' weights stand alone.

use serde::{Deserialize, Serialize};
use uniwrt_model::wifi::{ChannelWidth, WifiStandard};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Experience {
    pub score: u8,
    pub parts: Vec<ExperiencePart>,
}

/// One factor of the score: what it measured, its own score and its weight.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ExperiencePart {
    pub factor: Factor,
    pub score: u8,
    pub weight: u8,
    /// In the factor's unit: dB (SNR), dBm (signal), percent (PHY rate, retries, busy) or roams
    /// per hour.
    pub value: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Factor {
    Snr,
    Signal,
    PhyRate,
    Retries,
    ChannelBusy,
    Roaming,
}

/// What the score is made of, as the book measured it.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Inputs {
    pub(crate) signal_dbm: Option<i16>,
    pub(crate) noise_dbm: Option<i16>,
    /// The better of the last rates to and from the client, and the most it could reach;
    /// counted only while it moves traffic (an idle client's last frames are slow).
    pub(crate) rate_kbps: Option<u32>,
    pub(crate) capability_kbps: Option<u32>,
    pub(crate) active: bool,
    pub(crate) retry_percent: Option<f32>,
    pub(crate) failed_percent: Option<f32>,
    pub(crate) busy_percent: Option<f32>,
    pub(crate) roams_last_hour: Option<u32>,
}

pub(crate) fn experience(inputs: &Inputs) -> Option<Experience> {
    let mut parts = Vec::new();
    let mut part = |factor, weight, value: f32, score: f32| {
        parts.push(ExperiencePart {
            factor,
            // Clamped to 0–100 first.
            #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            score: score.round() as u8,
            weight,
            value,
        });
    };
    match (inputs.signal_dbm, inputs.noise_dbm) {
        (Some(signal), Some(noise)) if noise < signal => {
            let snr = f32::from(signal - noise);
            part(Factor::Snr, 35, snr, linear(snr, 10.0, 40.0));
        }
        (Some(signal), _) => {
            let signal = f32::from(signal);
            part(Factor::Signal, 35, signal, linear(signal, -85.0, -55.0));
        }
        _ => {}
    }
    if let (true, Some(rate), Some(max)) = (
        inputs.active,
        inputs.rate_kbps,
        inputs.capability_kbps.filter(|max| *max > 0),
    ) {
        #[expect(clippy::cast_precision_loss)]
        let percent = (rate as f32 / max as f32 * 100.0).min(100.0);
        part(Factor::PhyRate, 25, percent, linear(percent, 5.0, 50.0));
    }
    if let Some(retries) = inputs.retry_percent {
        let failed = inputs.failed_percent.unwrap_or(0.0);
        let cost = retries + failed * 5.0;
        part(Factor::Retries, 20, retries, linear(cost, 40.0, 5.0));
    }
    if let Some(busy) = inputs.busy_percent {
        part(Factor::ChannelBusy, 10, busy, linear(busy, 90.0, 40.0));
    }
    if let Some(roams) = inputs.roams_last_hour {
        #[expect(clippy::cast_precision_loss)]
        let roams = roams as f32;
        part(Factor::Roaming, 10, roams, linear(roams, 12.0, 4.0));
    }
    let weights: u32 = parts.iter().map(|part| u32::from(part.weight)).sum();
    // Signal or rates are needed: retries, air time and roams alone say little.
    if weights == 0
        || !parts
            .iter()
            .any(|part| matches!(part.factor, Factor::Snr | Factor::Signal | Factor::PhyRate))
    {
        return None;
    }
    let total: u32 = parts
        .iter()
        .map(|part| u32::from(part.weight) * u32::from(part.score))
        .sum();
    let score = u8::try_from((total + weights / 2) / weights).unwrap_or(100);
    Some(Experience { score, parts })
}

/// 0 at `bad`, 100 at `good`, straight between and clamped beyond; either may be the larger.
fn linear(value: f32, bad: f32, good: f32) -> f32 {
    ((value - bad) / (good - bad)).clamp(0.0, 1.0) * 100.0
}

/// The fastest PHY rate of a standard at a width, per spatial stream, in kb/s: the top MCS
/// with the shortest guard interval.
pub(crate) fn max_rate_kbps(standard: WifiStandard, width: ChannelWidth, streams: u8) -> u32 {
    use ChannelWidth::{Mhz20, Mhz40, Mhz80, Mhz160, Mhz320};
    let per_stream = match (standard, width) {
        (WifiStandard::Legacy, _) => 54_000,
        (WifiStandard::Ht, Mhz20) => 72_200,
        (WifiStandard::Ht, _) => 150_000,
        (WifiStandard::Vht, Mhz20) => 86_700,
        (WifiStandard::Vht, Mhz40) => 200_000,
        (WifiStandard::Vht, Mhz80) => 433_300,
        (WifiStandard::Vht, Mhz160 | Mhz320) => 866_700,
        (WifiStandard::He, Mhz20) => 143_400,
        (WifiStandard::He, Mhz40) => 286_800,
        (WifiStandard::He, Mhz80) => 600_500,
        (WifiStandard::He, Mhz160 | Mhz320) => 1_201_000,
        (WifiStandard::Eht, Mhz20) => 172_100,
        (WifiStandard::Eht, Mhz40) => 344_100,
        (WifiStandard::Eht, Mhz80) => 720_600,
        (WifiStandard::Eht, Mhz160) => 1_441_200,
        (WifiStandard::Eht, Mhz320) => 2_882_400,
    };
    per_stream * u32::from(streams.max(1))
}

#[cfg(test)]
mod tests {
    use uniwrt_model::wifi::{ChannelWidth, WifiStandard};

    use super::{Factor, Inputs, experience, max_rate_kbps};

    fn good() -> Inputs {
        Inputs {
            signal_dbm: Some(-50),
            noise_dbm: Some(-95),
            rate_kbps: Some(1_100_000),
            capability_kbps: Some(1_201_000),
            active: true,
            retry_percent: Some(2.0),
            failed_percent: Some(0.0),
            busy_percent: Some(20.0),
            roams_last_hour: Some(0),
        }
    }

    #[test]
    fn a_strong_quiet_client_scores_full_marks() {
        let experience = experience(&good()).unwrap();
        assert_eq!(experience.score, 100);
        assert_eq!(experience.parts.len(), 5);
        assert_eq!(experience.parts[0].factor, Factor::Snr);
        assert!((experience.parts[0].value - 45.0).abs() < f32::EPSILON);
    }

    #[test]
    fn each_factor_pulls_by_its_weight() {
        let weak = Inputs {
            signal_dbm: Some(-80),
            noise_dbm: Some(-90),
            ..good()
        };
        // SNR 10 dB scores 0, at weight 35 of 100.
        assert_eq!(experience(&weak).unwrap().score, 65);
        let flapping = Inputs {
            roams_last_hour: Some(20),
            ..good()
        };
        assert_eq!(experience(&flapping).unwrap().score, 90);
        let failing = Inputs {
            retry_percent: Some(10.0),
            failed_percent: Some(4.0),
            ..good()
        };
        // 10 % + 5 × 4 % = 30 %: 29 points of 100 for retries.
        assert_eq!(experience(&failing).unwrap().score, 86);
    }

    #[test]
    fn missing_parts_drop_out() {
        let idle = Inputs {
            active: false,
            noise_dbm: None,
            retry_percent: None,
            busy_percent: None,
            roams_last_hour: None,
            signal_dbm: Some(-70),
            ..good()
        };
        let experience = experience(&idle).unwrap();
        assert_eq!(experience.parts.len(), 1);
        assert_eq!(experience.parts[0].factor, Factor::Signal);
        assert_eq!(experience.score, 50);
        let blind = Inputs {
            signal_dbm: None,
            active: false,
            ..good()
        };
        assert_eq!(super::experience(&blind), None);
    }

    #[test]
    fn rates_scale_with_width_and_streams() {
        assert_eq!(
            max_rate_kbps(WifiStandard::He, ChannelWidth::Mhz80, 2),
            1_201_000
        );
        assert_eq!(
            max_rate_kbps(WifiStandard::Legacy, ChannelWidth::Mhz80, 0),
            54_000
        );
    }
}
