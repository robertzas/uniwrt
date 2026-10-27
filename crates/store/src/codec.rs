//! Column encodings shared by the repositories: enums as serde names, MACs as canonical text,
//! timestamps as Unix milliseconds, and constraint violations as the domain errors they mean.

use std::fmt::Display;

use rusqlite::{Row, ffi, types::Type};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use uniwrt_core::{MacAddr, Timestamp};

use crate::Error;

/// The serde name of a unit enum variant, such as `observe` for `NodeMode::Observe`.
pub(crate) fn enum_text(value: impl Serialize) -> String {
    let Ok(Value::String(text)) = serde_json::to_value(value) else {
        unreachable!("callers pass unit enum variants, which serialize as strings");
    };
    text
}

pub(crate) fn enum_column<T: DeserializeOwned>(row: &Row<'_>, index: usize) -> rusqlite::Result<T> {
    let text: String = row.get(index)?;
    serde_json::from_value(Value::String(text)).map_err(|err| invalid(index, err))
}

pub(crate) fn mac_column(row: &Row<'_>, index: usize) -> rusqlite::Result<MacAddr> {
    row.get::<_, String>(index)?
        .parse()
        .map_err(|err| invalid(index, err))
}

pub(crate) fn timestamp_column(row: &Row<'_>, index: usize) -> rusqlite::Result<Timestamp> {
    row.get(index).map(Timestamp::from_unix_ms)
}

fn invalid(index: usize, err: impl std::error::Error + Send + Sync + 'static) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(index, Type::Text, Box::new(err))
}

/// Maps a constraint violation on `what` `key` to its meaning: a UNIQUE violation is a conflict,
/// a foreign key violation a missing referenced row. Other errors pass through.
pub(crate) fn constraint(err: rusqlite::Error, what: &'static str, key: impl Display) -> Error {
    match err.sqlite_error().map(|err| err.extended_code) {
        Some(ffi::SQLITE_CONSTRAINT_UNIQUE) => Error::Conflict {
            what,
            key: key.to_string(),
        },
        Some(ffi::SQLITE_CONSTRAINT_FOREIGNKEY) => Error::NotFound {
            what,
            key: key.to_string(),
        },
        _ => err.into(),
    }
}
