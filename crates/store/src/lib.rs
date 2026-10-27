//! SQLite persistence: one database in WAL mode, a batching writer thread, a read pool and repositories.
//!
//! - **API:** [`Store::open`] creates or migrates the database and returns a cloneable handle;
//!   [`Store::close`] commits what is queued and checkpoints. Repositories are traits that `Store`
//!   implements: [`NodeRepo`] (the node inventory), [`FactsRepo`] (the latest facts snapshot of
//!   each node), [`HostKeyRepo`] (the SSH host key pinned for each node) and [`VaultRepo`]
//!   (secrets by name; a stub that stores them unencrypted until M1.6). Errors are [`Error`].
//! - **Writes** go to one thread that owns the only write connection. It takes every queued
//!   operation (up to 256) into one transaction and runs each in its own savepoint, so an operation
//!   that fails or panics rolls back alone. Callers hear back once the batch has committed; under
//!   load, many writes share one commit. A full queue (1,024) makes callers wait.
//! - **Reads** run on tokio's blocking pool over up to 4 read-only connections, opened on demand;
//!   WAL lets them run while the writer commits.
//! - **Schema:** forward-only migrations embedded from `migrations/NNNN_name.sql` and tracked in
//!   `PRAGMA user_version`; a database from a newer uniwrt is refused ([`Error::TooNew`]). Never
//!   edit a released migration: add a new one.
//! - **Invariants:** SQL stays inside this crate. The database file is created with mode 0600
//!   (SQLite gives the WAL the same mode). Foreign keys are enforced. `synchronous=NORMAL`: a
//!   power loss may lose the last commits but never corrupts the database.
//! - **Dependencies:** `uniwrt-core`, `uniwrt-model`, rusqlite with bundled SQLite (FTS5 included),
//!   tokio (sync and the blocking pool), `serde_json` for JSON columns, `zeroize` for secrets.
//! - **Tests:** each test opens a fresh database in a temporary directory
//!   (`cargo nextest run -p uniwrt-store`).

mod codec;
mod connection;
mod error;
mod migrate;
mod readers;
mod repo;
mod store;
#[cfg(test)]
mod testing;
mod writer;

pub use error::{Error, Result};
pub use repo::{FactsRepo, HostKeyRepo, NodeRepo, Secret, VaultRepo};
pub use store::Store;
