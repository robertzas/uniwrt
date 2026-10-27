//! Store errors: opening and migrating, domain outcomes (conflict, not found) and failed writes.

use std::{io, path::PathBuf, sync::Arc};

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("cannot create the database file {}", path.display())]
    Create {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("cannot open the database {}", path.display())]
    Open {
        path: PathBuf,
        #[source]
        source: rusqlite::Error,
    },
    #[error(
        "the database {} cannot use WAL mode (journal mode {mode}); is it on a network filesystem?",
        path.display()
    )]
    NoWal { path: PathBuf, mode: String },
    #[error(
        "the database schema is at version {found}, newer than this uniwrt knows ({supported}); \
         run a newer uniwrt"
    )]
    TooNew { found: i64, supported: usize },
    #[error("database migration {name} failed")]
    Migration {
        name: &'static str,
        #[source]
        source: rusqlite::Error,
    },
    #[error("cannot start the database writer")]
    Writer(#[source] io::Error),
    #[error("{what} {key} already exists")]
    Conflict { what: &'static str, key: String },
    #[error("{what} {key} does not exist")]
    NotFound { what: &'static str, key: String },
    #[error("cannot encode {what}")]
    Encode {
        what: &'static str,
        #[source]
        source: serde_json::Error,
    },
    #[error("stored {what} of {key} cannot be decoded")]
    Decode {
        what: &'static str,
        key: String,
        #[source]
        source: serde_json::Error,
    },
    /// The write succeeded inside its batch, but the batch did not commit.
    #[error("the write batch did not commit")]
    Commit(#[source] Arc<rusqlite::Error>),
    #[error("the store is closed")]
    Closed,
    /// The operation panicked, or the thread running it died.
    #[error("the database operation was aborted")]
    Aborted,
    #[error("database error")]
    Sqlite(#[from] rusqlite::Error),
}
