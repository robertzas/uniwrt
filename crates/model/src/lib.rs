//! The uniwrt domain model: what the controller observes on nodes and what it records about them.
//!
//! - **API:** [`facts`] holds the observed state of one node ([`facts::NodeFacts`]: board, system,
//!   capabilities, interfaces, devices, phys, radios, stations, DHCP leases, neighbors, the bridge
//!   FDB, batman-adv meshes), which the `node` crate parses agent output into. [`node`] is the inventory record ([`node::Node`]: identity, address, role, mode)
//!   and the pinned SSH host key ([`node::HostKeyPin`]).
//!   [`wifi`] is the Wi-Fi vocabulary shared by facts and, later, desired state (`SiteConfig`).
//! - **Wire format:** serde with Rust field names (`snake_case`), units in field names (`_dbm`,
//!   `_mhz`, `_kbps`, `_bytes`, `_s`, `_ms`), `null` for unknown values rather than omitted fields.
//!   The same JSON is stored in SQLite and sent to the UI.
//! - **Invariants:** L0 crate: pure data, no tokio and no IO. Facts describe what was observed and
//!   never what should be; rendering decides from [`facts::Capabilities`], not version strings.
//!   Facts are persisted, so a field added later must be an `Option` or `#[serde(default)]` for
//!   older snapshots to keep parsing; renaming or removing a field needs a store migration.
//! - **Dependencies:** `uniwrt-core`, `serde`.
//! - **Tests:** serde names per type, and canonical fixtures in `fixtures/facts/*.json` that must
//!   round-trip without losing a field (`cargo nextest run -p uniwrt-model`). The fixtures double as
//!   wire-format examples for the simulator and the UI.

pub mod facts;
pub mod node;
pub mod wifi;
