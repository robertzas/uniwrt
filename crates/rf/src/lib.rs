//! RF insight (features/wifi.md, RF insight): what the site's radios hear and how crowded their
//! channels are, from the nodes' facts and their radios' neighbor scans; later the channel planner
//! (M6.1).
//!
//! - **API:** [`report`] builds an [`RfReport`] from [`Inputs`] (each node's facts, each radio's
//!   latest scan, the radar history): per band, the radios with the foreign networks on their
//!   channel ([`Overlap`]) and their interference, each 20 MHz channel's occupancy
//!   ([`ChannelReport`]: allowed, DFS state, radar hits, networks, pressure, busy share, noise,
//!   the site's radios on it), every network heard ([`Neighbor`]: own, lookalike or foreign, its
//!   vendor and who heard it) and the inter-node RSSI matrix ([`MatrixEntry`]: live mesh links,
//!   else scans). [`Span`] is the frequencies a channel occupies and [`Span::overlap`] how much
//!   two share; [`weight`] how much a signal counts (−90 dBm ≈ 0, −60 dBm ≈ 1).
//!   [`RfView`] is what the API sends: the report with each radio's scanning state.
//!   [`radar::span`] reads the channel of a `DFS-RADAR-DETECTED` log line ([`RadarHit`]).
//! - **Invariants:** pure and deterministic: no IO and no clock. A network's impact on a radio
//!   is `weight(signal) × overlap`, with the signal its own node heard; networks the radio's node
//!   did not hear do not count against it. Numbers are rounded to two decimals.
//! - **Dependencies:** `uniwrt-core`, `uniwrt-model`, serde; ts-rs with the `ts` feature.
//! - **Tests:** next to each concept and in `tests.rs` over a small site made in code
//!   (`cargo nextest run -p uniwrt-rf`).

mod build;
pub mod radar;
mod report;
mod span;

pub use self::{
    build::{Inputs, report},
    radar::RadarHit,
    report::{
        BandReport, ChannelReport, ChannelUse, Hearing, MatrixEntry, MatrixSource, Neighbor,
        NeighborKind, Overlap, RadioReport, RfReport, RfView,
    },
    span::{Span, weight},
};
