//! Metrics (features/observability.md, Metrics): what the controller records as series, and how
//! it reads them back.
//!
//! - **API:** [`Sampler`] turns what the engine announces into [`Sample`]s: each node's facts
//!   ([`Sampler::facts`]: CPU, memory, load and temperature; each device's traffic from its
//!   counters; each radio's busy share, noise and clients), each topology build
//!   ([`Sampler::topology`]: WAN traffic, clients and nodes online, mesh links' signal, TQ,
//!   quality and capacity) and the clients' samples ([`Sampler::clients`]). It averages what
//!   arrives in aligned [`WINDOW`]s and hands closed windows out with [`Sampler::drain`].
//!   [`Retention`] says how long each [`Resolution`] is kept and [`Retention::resolution`] which
//!   one answers a query; [`downsample`] reduces a series to a chart's width with LTTB, keeping
//!   each kept point's envelope and breaking the line where the data stops.
//! - **Invariants:** pure and deterministic: no IO and no clock (times come from the facts, the
//!   builds and the samples). A counter that went backwards (a reboot, a wrap) gives no rate
//!   for that interval. Rates are averaged over time within a window, gauges over readings.
//! - **Dependencies:** `uniwrt-core`, `uniwrt-model`, `uniwrt-topo`, `uniwrt-clients`.
//! - **Tests:** next to each concept (`cargo nextest run -p uniwrt-metrics`).

// Values are f64 for charts: byte counters, counts and times past 2^52 lose their last digits,
// which no chart shows.
#![allow(clippy::cast_precision_loss)]

mod downsample;
mod retention;
mod sampler;

pub use self::{
    downsample::{Downsampled, downsample},
    retention::Retention,
    sampler::{Sampler, WINDOW},
};
pub use uniwrt_model::metric::{Entity, Metric, Point, Resolution, Sample, Unit};
