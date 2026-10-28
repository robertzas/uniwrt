//! The `system` set: `system info` (memory in bytes, filesystems in KiB, loads scaled by
//! 65536), the node's clock, the agent's CPU percentage and the thermal zones (m°C).

use serde::Deserialize;
use uniwrt_core::Timestamp;
use uniwrt_model::facts::{Memory, Storage, SystemFacts};

use super::round;

/// Thermal zones outside this range (°C) are broken sensors.
const PLAUSIBLE_C: std::ops::RangeInclusive<f64> = -40.0..=150.0;

#[derive(Deserialize)]
pub(super) struct Raw {
    info: Info,
    /// `clock()`: seconds and nanoseconds.
    #[serde(default)]
    clock: Option<(i64, i64)>,
    #[serde(default)]
    cpu_percent: Option<f64>,
    #[serde(default)]
    thermal: Vec<Zone>,
}

#[derive(Deserialize)]
struct Info {
    uptime: u64,
    #[serde(default)]
    load: [u64; 3],
    memory: Mem,
    #[serde(default)]
    root: Option<Fs>,
    #[serde(default)]
    tmp: Option<Fs>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Mem {
    total: u64,
    free: u64,
    buffered: u64,
    available: u64,
    cached: u64,
}

#[derive(Deserialize)]
struct Fs {
    total: u64,
    used: u64,
    avail: u64,
}

impl Fs {
    fn storage(&self) -> Storage {
        Storage {
            total_bytes: self.total.saturating_mul(1024),
            used_bytes: self.used.saturating_mul(1024),
            available_bytes: self.avail.saturating_mul(1024),
        }
    }
}

#[derive(Deserialize)]
struct Zone {
    temp: i64,
}

#[expect(
    clippy::cast_precision_loss,
    reason = "loads and temperatures are far below 2^52"
)]
pub(super) fn parse(raw: Raw) -> SystemFacts {
    let Raw {
        info,
        clock,
        cpu_percent,
        thermal,
    } = raw;
    // Two decimals, as `uptime` prints them; the kernel keeps 11 fractional bits.
    let load = info.load.map(|load| round(load as f64 / 65536.0, 2));
    SystemFacts {
        uptime_s: info.uptime,
        node_time: clock.map(|(secs, nanos)| {
            Timestamp::from_unix_ms(secs.saturating_mul(1000).saturating_add(nanos / 1_000_000))
        }),
        load,
        cpu_percent: cpu_percent.map(|percent| round(percent.clamp(0.0, 100.0), 1)),
        memory: Memory {
            total_bytes: info.memory.total,
            available_bytes: info.memory.available,
            free_bytes: info.memory.free,
            cached_bytes: info.memory.cached,
            buffered_bytes: info.memory.buffered,
        },
        root: info.root.as_ref().map(Fs::storage),
        tmp: info.tmp.as_ref().map(Fs::storage),
        temperature_c: thermal
            .iter()
            .map(|zone| zone.temp as f64 / 1000.0)
            .filter(|celsius| PLAUSIBLE_C.contains(celsius))
            .reduce(f64::max)
            .map(|celsius| round(celsius, 1)),
    }
}
