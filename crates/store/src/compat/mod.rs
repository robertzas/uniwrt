//! The 1.x compatibility promise for the database (features/lifecycle.md, Compatibility;
//! D-152): any later 1.x opens the data directory of any earlier one. Two tests hold it:
//!
//! - **Frozen migrations:** `migrations/frozen.sha256` lists the SHA-256 of every migration a
//!   release has shipped. One that changes or goes fails the test, and a new one fails it until
//!   its line is appended (the failure gives the line).
//! - **Fixtures:** `fixtures/compat/schema-<N>.sql` holds the rows [`seed::seed`] wrote at
//!   schema N, as `INSERT`s ([`dump`]). Each is loaded into a database migrated to N
//!   ([`load`]), opened by [`Store::open`](crate::Store::open), which migrates it to the
//!   latest schema, and read back through the repositories ([`seed::check`]). The latest schema
//!   must have one: `UNIWRT_BLESS=1` writes it. A fixture is never rewritten and the seed's
//!   constants never change, so every fixture keeps being read by every later uniwrt.

use std::{fs, path::Path};

use rusqlite::{Connection, types::ValueRef};

mod seed;
#[cfg(test)]
mod tests;

/// The rows of every table `conn` holds, as `INSERT` statements sorted within each table, the
/// tables by name. Virtual tables and their shadow tables (the logs' FTS5 index) are left out:
/// their rows follow from the others'.
pub(crate) fn dump(conn: &Connection) -> rusqlite::Result<String> {
    let tables = conn
        .prepare(
            "SELECT name FROM pragma_table_list
             WHERE schema = 'main' AND type = 'table' AND name NOT LIKE 'sqlite_%'
             ORDER BY name",
        )?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut out = String::new();
    for table in tables {
        let mut statement = conn.prepare(&format!("SELECT * FROM \"{table}\""))?;
        let columns = statement.column_count();
        let mut rows = Vec::new();
        let mut query = statement.query([])?;
        while let Some(row) = query.next()? {
            let values = (0..columns)
                .map(|column| row.get_ref(column).map(literal))
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows.push(format!(
                "INSERT INTO \"{table}\" VALUES ({});\n",
                values.join(", ")
            ));
        }
        rows.sort();
        out.extend(rows);
    }
    Ok(out)
}

/// Creates the database at `path` at schema `schema` and inserts `rows` (a [`dump`]).
pub(crate) fn load(path: &Path, schema: usize, rows: &str) -> crate::Result<()> {
    let mut conn = Connection::open(path)?;
    crate::migrate::run_to(&mut conn, schema)?;
    // The rows go in by table name, children before their parents at times: the test checks
    // the foreign keys once the store has opened the database.
    conn.pragma_update(None, "foreign_keys", false)?;
    let tx = conn.transaction()?;
    tx.execute_batch(rows)?;
    tx.commit()?;
    Ok(())
}

/// The fixtures in `dir` by schema, oldest first.
pub(crate) fn fixtures(dir: &Path) -> Vec<(usize, String)> {
    let mut found: Vec<(usize, String)> = fs::read_dir(dir).map_or_default(|entries| {
        entries
            .filter_map(|entry| {
                let name = entry.ok()?.file_name().into_string().ok()?;
                let schema = name.strip_prefix("schema-")?.strip_suffix(".sql")?;
                Some((schema.parse().ok()?, name))
            })
            .collect()
    });
    found.sort();
    found
}

fn literal(value: ValueRef<'_>) -> String {
    match value {
        ValueRef::Null => "NULL".to_owned(),
        ValueRef::Integer(n) => n.to_string(),
        ValueRef::Real(x) => format!("{x:?}"),
        ValueRef::Text(text) => format!("'{}'", String::from_utf8_lossy(text).replace('\'', "''")),
        ValueRef::Blob(bytes) => format!("X'{}'", data_encoding::HEXUPPER.encode(bytes)),
    }
}
