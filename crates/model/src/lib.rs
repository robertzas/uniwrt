//! The uniwrt domain model: what the controller observes on nodes, what it records about them,
//! and the network it should make of them.
//!
//! - **API:** [`facts`] holds the observed state of one node ([`facts::NodeFacts`]: board, system,
//!   capabilities, interfaces, devices, phys, radios, stations, DHCP leases, neighbors, the bridge
//!   FDB, batman-adv meshes), which the `node` crate parses agent output into. [`node`] is the inventory record ([`node::Node`]: identity, address, role, mode)
//!   and the pinned SSH host key ([`node::HostKeyPin`]).
//!   [`wifi`] is the Wi-Fi vocabulary shared by facts and, later, desired state (`SiteConfig`).
//!   [`event`] is what nodes report as it happens ([`event::NodeEvent`]: stations joining and
//!   leaving, devices and interfaces changing, DHCP leases) plus the roams the engine infers from
//!   them ([`event::Roam`]), and [`log`] their log entries. [`events`] is the timeline the
//!   controller records ([`events::EventRecord`]: nodes, WAN, mesh, clients, annotated log
//!   entries, failed jobs). [`status`] is a node's status as the
//!   engine derives it ([`status::NodeStatus`]: online, degraded or offline, with reasons).
//!   [`user`] holds the controller's users, their sessions and the audit log's entries.
//!   [`job`] is what the controller does on nodes step by step ([`job::Job`], adoption first),
//!   and [`inventory`] what it keeps about a node besides facts: installed packages
//!   ([`inventory::Inventory`]) and configuration backups ([`inventory::Backup`]).
//!   [`client`] is what the controller keeps about clients ([`client::ClientRecord`]) and where
//!   they were connected ([`client::ClientSession`]).
//!   [`metric`] names metric series ([`metric::Entity`], [`metric::Metric`] with its
//!   [`metric::Unit`]) and carries their [`metric::Sample`]s and [`metric::Point`]s at a
//!   [`metric::Resolution`].
//!   [`alerts`] holds alert rules ([`alerts::AlertRule`] over metrics or events), the alerts
//!   they fire ([`alerts::AlertRecord`]) and maintenance windows.
//!   [`rf`] is what radios' neighbor scans hear ([`rf::Scan`]: networks and the channel survey)
//!   and the scanning settings ([`rf::RfSettings`]).
//!   [`site`] is the desired state: the site's document ([`site::SiteConfig`]), its validation,
//!   structural diff and schema migrations, and its revisions.
//!   [`speed`] is a speed test from the gateway ([`speed::SpeedTest`]: rates, latency idle and
//!   under load, the bufferbloat [`speed::Grade`]) and what it suggests for SQM.
//!   [`discovery`] is what discovery reports: unadopted OpenWrt devices the adopted nodes see
//!   ([`discovery::Discovered`]); [`ztp`] is zero-touch provisioning's hello, pending inbox and
//!   pre-approvals (D-102); [`firmware`] is OpenWrt's catalog and the zero-touch images ASU builds
//!   (D-104).
//! - **Wire format:** serde with Rust field names (`snake_case`), units in field names (`_dbm`,
//!   `_mhz`, `_kbps`, `_bytes`, `_s`, `_ms`), `null` for unknown values rather than omitted fields.
//!   The same JSON is stored in SQLite and sent to the UI.
//! - **Invariants:** L0 crate: pure data, no tokio and no IO. Facts describe what was observed and
//!   never what should be; rendering decides from [`facts::Capabilities`], not version strings.
//!   Facts are persisted, so a field added later must be an `Option` or `#[serde(default)]` for
//!   older snapshots to keep parsing; renaming or removing a field needs a store migration.
//! - **Dependencies:** `uniwrt-core`, `uniwrt-uci` (UCI identifier rules), `serde`,
//!   `serde_json` (the site's diff and migrations), `thiserror`.
//! - **Tests:** serde names per type, and canonical fixtures in `fixtures/facts/*.json` that must
//!   round-trip without losing a field (`cargo nextest run -p uniwrt-model`). The fixtures double as
//!   wire-format examples for the simulator and the UI.

pub mod alerts;
pub mod apply;
pub mod client;
pub mod discovery;
pub mod drift;
pub mod event;
pub mod events;
pub mod facts;
pub mod firmware;
pub mod import;
pub mod inventory;
pub mod job;
pub mod log;
pub mod metric;
pub mod node;
pub mod notify;
pub mod rf;
pub mod site;
pub mod speed;
pub mod status;
pub mod user;
pub mod wifi;
pub mod ztp;
