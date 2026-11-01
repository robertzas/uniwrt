//! Clients (features/observability.md, Clients): the inventory of every device that connects
//! through the adopted nodes, who each one is, where it was connected and how well.
//!
//! - **API:** [`Book`] keeps every client the controller knows: [`Book::update`] takes each
//!   topology build (with the [`Member`](uniwrt_topo::Member)s it was built from),
//!   [`Book::restore`] what the store kept, [`Book::rename`] a user's alias, [`Book::prune`]
//!   forgets clients long gone; each returns its [`Changes`]. [`Book::clients`] and
//!   [`Book::detail`] read it. A client now is a [`Client`] (identity, [`Connection`],
//!   [`Experience`]); its recent history is [`ClientSample`]s. [`vendor`] names the
//!   organization a MAC address is assigned to; [`classify`] guesses a [`ClientKind`].
//! - **How:** a client is a `client:<mac>` vertex of the topology; its edge gives its place
//!   (an access point's interface, or a node's port, or the port an inferred switch hangs
//!   from) and the node's facts its station and radio. A client is online while its link is up
//!   and that node is not offline. Sessions follow its place: moving while connected (or
//!   showing up elsewhere within the roam window) is a roam, disappearing ends one at the last
//!   time it was seen, and an association that began before its session ended (the station's
//!   connected time says) carries on the same session, as after a restart. Randomized
//!   addresses are grouped by hostname, else by a DHCP client ID that is not the address, and
//!   share an alias. The experience score v1 is in `score.rs`; the vendor table (IEEE MA-L,
//!   MA-M, MA-S) is embedded and made by `scripts/oui.py` (D-053).
//! - **Invariants:** pure and deterministic: no IO, no clock (times come from the builds and
//!   the facts). Sessions of a client never overlap, and a session's start identifies it. A
//!   record's alias changes only through [`Book::rename`]; the store keeps it.
//! - **Dependencies:** `uniwrt-core`, `uniwrt-model`, `uniwrt-topo`, serde, `miniz_oxide`
//!   (inflating the vendor table); ts-rs behind the `ts` feature.
//! - **Tests:** unit tests per module; `tests/` drives a book through builds of scenarios
//!   made in code (`cargo nextest run -p uniwrt-clients`).

mod book;
mod entry;
mod kind;
mod oui;
mod rules;
mod score;
#[cfg(test)]
mod tests;
mod view;

pub use book::{Book, BookConfig, Changes, Detail};
pub use kind::{ClientKind, classify};
pub use oui::vendor;
pub use score::{Experience, ExperiencePart, Factor};
pub use view::{Client, ClientSample, Connection};
