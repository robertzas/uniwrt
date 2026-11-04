//! Schema migrations: forward-only steps that bring a stored document to [`SCHEMA_VERSION`].
//! Revisions keep the JSON they were written with; reading one upgrades it.
//!
//! To change the schema: raise [`SCHEMA_VERSION`], append a step to [`STEPS`] that rewrites the
//! JSON of the previous version, and add `fixtures/site/v<new>.json` to the tests' `FIXTURES`,
//! which upgrade the fixture of every version.

use serde_json::Value;

use super::{SCHEMA_VERSION, SiteConfig};

/// Rewrites the JSON of one version into the next: `STEPS[0]` turns v1 into v2, and so on.
type Step = fn(&mut Value) -> Result<(), String>;

const STEPS: &[Step] = &[];

const _: () = assert!(STEPS.len() + 1 == SCHEMA_VERSION as usize);

/// Why a stored document cannot be read.
#[derive(Debug, thiserror::Error)]
pub enum SchemaError {
    #[error("the document has no schema version")]
    Missing,
    #[error("the document's schema {found} is newer than this uniwrt knows ({SCHEMA_VERSION})")]
    TooNew { found: u64 },
    #[error("upgrading the document from schema {from} failed: {reason}")]
    Step { from: u64, reason: String },
    #[error("the document does not match schema {SCHEMA_VERSION}")]
    Invalid(#[source] serde_json::Error),
}

/// The document `value`, upgraded to the current schema.
pub fn upgrade(mut value: Value) -> Result<SiteConfig, SchemaError> {
    let found = value
        .get("schema")
        .and_then(Value::as_u64)
        .filter(|&found| found >= 1)
        .ok_or(SchemaError::Missing)?;
    if found > u64::from(SCHEMA_VERSION) {
        return Err(SchemaError::TooNew { found });
    }
    for (from, step) in (found..).zip(STEPS.iter().skip(usize::try_from(found - 1).unwrap_or(0))) {
        step(&mut value).map_err(|reason| SchemaError::Step { from, reason })?;
        if let Some(schema) = value.get_mut("schema") {
            *schema = (from + 1).into();
        }
    }
    serde_json::from_value(value).map_err(SchemaError::Invalid)
}
