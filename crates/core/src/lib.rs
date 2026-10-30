//! Shared vocabulary of every uniwrt crate: IDs, MAC and IP types, timestamps and the `Clock` trait.
//!
//! - **API:** [`NodeId`] and [`UserId`] (store-assigned, never reused); [`MacAddr`] (EUI-48, canonical
//!   `aa:bb:cc:dd:ee:ff`); [`IpNet`], [`Ipv4Net`] and [`Ipv6Net`] (re-exported from `ipnet`: an
//!   address plus a prefix length, host bits kept, as in `10.0.0.1/24`); [`Timestamp`] (Unix
//!   milliseconds, UTC); [`Clock`] with [`SystemClock`] and [`ManualClock`].
//! - **Wire format:** IDs and timestamps serialize as JSON integers; MACs and prefixes as strings.
//!   Parsing is lenient where OpenWrt tools differ (MAC case and separators), output is canonical.
//! - **Error conventions** (all crates): each library crate has its own `thiserror` enum with
//!   context fields (what, which key, which path) and `#[source]` causes, plus a `Result` alias;
//!   only the binary uses `anyhow`. Parse errors of core types are their own small types here.
//! - **Invariants:** L0 crate (ARCHITECTURE.md): no tokio and no IO. [`SystemClock`] is the only
//!   wall-clock read in L0; only composition roots construct it, everything else takes a `Clock`.
//! - **Dependencies:** `serde`, `thiserror`, `ipnet`.
//! - **Tests:** unit and property tests (`proptest`) next to each type; `cargo nextest run -p
//!   uniwrt-core`.

mod clock;
mod id;
mod mac;
mod time;

pub use clock::{Clock, ManualClock, SystemClock};
pub use id::{NodeId, UserId};
pub use ipnet::{IpNet, Ipv4Net, Ipv6Net};
pub use mac::{MacAddr, ParseMacError};
pub use time::Timestamp;
