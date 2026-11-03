//! Parsing the agent's `scan` answer ([`Scanned`]) into what the radio heard ([`Heard`]).

use std::collections::{BTreeMap, HashMap};

use data_encoding::BASE64;
use uniwrt_core::MacAddr;
use uniwrt_model::{
    rf::{Bss, ChannelSurvey, Heard},
    wifi::{Band, WifiStandard},
};

use super::{ies::Elements, round};
use crate::proto::ops::{RawBss, RawSurvey, Scanned};

/// Results the kernel last heard this long before the scan began are left out: they are cached
/// from an earlier scan (a driver that ignores the flush).
const STALE_SLACK_MS: i64 = 1_000;

/// What a radio of `band` heard. Networks heard twice keep their strongest signal; channels of
/// other bands (a phy serving several) are left out.
pub fn heard(raw: &Scanned, band: Band) -> Heard {
    let duration_ms = u32::try_from((raw.ended_ms - raw.started_ms).max(0)).unwrap_or(u32::MAX);
    let mut bsss: HashMap<MacAddr, Bss> = HashMap::new();
    for raw_bss in &raw.bss {
        if raw_bss
            .seen_ms_ago
            .is_some_and(|ago| ago > i64::from(duration_ms) + STALE_SLACK_MS)
        {
            continue;
        }
        let Some(bss) = bss(raw_bss, band) else {
            continue;
        };
        match bsss.get(&bss.bssid) {
            Some(known) if known.signal_dbm >= bss.signal_dbm => {}
            _ => {
                bsss.insert(bss.bssid, bss);
            }
        }
    }
    let mut bsss: Vec<Bss> = bsss.into_values().collect();
    bsss.sort_by(|a, b| {
        (a.frequency_mhz, b.signal_dbm, a.bssid).cmp(&(b.frequency_mhz, a.signal_dbm, b.bssid))
    });
    Heard {
        duration_ms,
        bsss,
        channels: channels(&raw.survey_before, &raw.survey_after, band),
    }
}

fn bss(raw: &RawBss, band: Band) -> Option<Bss> {
    let bssid: MacAddr = raw.bssid.parse().ok()?;
    if Band::of_frequency(raw.frequency) != Some(band) {
        return None;
    }
    let signal_dbm = i16::try_from(raw.signal_mbm? / 100).ok()?;
    let bytes = raw
        .ies
        .as_deref()
        .and_then(|ies| BASE64.decode(ies.as_bytes()).ok())
        .unwrap_or_default();
    let elements = Elements::parse(&bytes);
    // The element's channel names the BSS's own channel even when a neighbor channel heard it
    // (2.4 GHz channels overlap).
    let channel = elements
        .channel
        .filter(|&c| Band::of_frequency(band.frequency(c)) == Some(band))
        .or_else(|| band.channel(raw.frequency))?;
    let frequency_mhz = band.frequency(channel);
    let (width, center_frequency_mhz) = elements.width(band, frequency_mhz);
    Some(Bss {
        bssid,
        ssid: name(elements.ssid.as_deref()),
        mesh_id: name(elements.mesh_id.as_deref()),
        channel,
        frequency_mhz,
        center_frequency_mhz,
        width,
        signal_dbm,
        standard: elements.standard.unwrap_or(WifiStandard::Legacy),
        security: elements.security(raw.capability),
        stations: elements.stations,
        utilization_percent: elements.utilization_percent,
    })
}

/// A network's name; `None` when hidden (empty or all NULs).
fn name(bytes: Option<&[u8]>) -> Option<String> {
    let bytes = bytes?;
    if bytes.iter().all(|&b| b == 0) {
        return None;
    }
    Some(String::from_utf8_lossy(bytes).into_owned())
}

/// Each channel's survey while the scan listened: the counters after it less those before it,
/// or the counters after it when there were none before (drivers that start them per scan).
fn channels(before: &[RawSurvey], after: &[RawSurvey], band: Band) -> Vec<ChannelSurvey> {
    let before: HashMap<u32, &RawSurvey> = before.iter().map(|s| (s.frequency, s)).collect();
    let mut channels = BTreeMap::new();
    for survey in after {
        let Some(channel) = Band::of_frequency(survey.frequency)
            .filter(|&b| b == band)
            .and_then(|b| b.channel(survey.frequency))
        else {
            continue;
        };
        let (time, busy) = match (before.get(&survey.frequency), survey.time, survey.busy) {
            (Some(b), Some(time), busy) if b.time.is_some_and(|t| t <= time) => (
                time - b.time.unwrap_or_default(),
                busy.zip(b.busy)
                    .map(|(after, before)| after.saturating_sub(before)),
            ),
            (_, Some(time), busy) => (time, busy),
            (_, None, _) => (0, None),
        };
        #[allow(clippy::cast_precision_loss)]
        let busy_percent = busy
            .filter(|_| time > 0)
            .map(|busy| round((100.0 * busy as f64 / time as f64).min(100.0), 1));
        channels.insert(
            survey.frequency,
            ChannelSurvey {
                channel,
                frequency_mhz: survey.frequency,
                noise_dbm: survey.noise.and_then(|n| i16::try_from(n).ok()),
                busy_percent,
                dwell_ms: (time > 0).then(|| u32::try_from(time).unwrap_or(u32::MAX)),
            },
        );
    }
    channels.into_values().collect()
}

#[cfg(test)]
#[path = "tests/scan.rs"]
mod tests;
