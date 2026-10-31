//! Forward-only schema migrations, embedded at build time and tracked in `PRAGMA user_version`.

use rusqlite::Connection;
use tracing::info;

use crate::{Error, Result};

struct Migration {
    name: &'static str,
    sql: &'static str,
}

macro_rules! migrations {
    ($($name:literal),* $(,)?) => {
        &[$(Migration { name: $name, sql: include_str!(concat!("../migrations/", $name, ".sql")) }),*]
    };
}

/// Migration N (1-based) brings the schema from version N-1 to N. Append only.
const MIGRATIONS: &[Migration] = migrations![
    "0001_nodes",
    "0002_vault_host_keys",
    "0003_users_sessions_audit",
    "0004_jobs_inventory_backups",
    "0005_topology",
];

/// Applies the migrations the database lacks, each in its own transaction.
pub(crate) fn run(conn: &mut Connection) -> Result<()> {
    let found: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    let applied = usize::try_from(found)
        .ok()
        .filter(|&applied| applied <= MIGRATIONS.len())
        .ok_or(Error::TooNew {
            found,
            supported: MIGRATIONS.len(),
        })?;
    for (version, migration) in (1_i64..).zip(MIGRATIONS).skip(applied) {
        let failed = |source| Error::Migration {
            name: migration.name,
            source,
        };
        let tx = conn.transaction().map_err(failed)?;
        tx.execute_batch(migration.sql).map_err(failed)?;
        tx.pragma_update(None, "user_version", version)
            .map_err(failed)?;
        tx.commit().map_err(failed)?;
        info!(migration = migration.name, "applied a database migration");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::{MIGRATIONS, run};
    use crate::Error;

    fn version(conn: &Connection) -> i64 {
        conn.query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn names_are_numbered_in_order() {
        for (index, migration) in MIGRATIONS.iter().enumerate() {
            let prefix = format!("{:04}_", index + 1);
            assert!(migration.name.starts_with(&prefix), "{}", migration.name);
        }
    }

    #[test]
    fn a_fresh_database_reaches_the_latest_version_and_reruns_are_no_ops() {
        let mut conn = Connection::open_in_memory().unwrap();
        run(&mut conn).unwrap();
        assert_eq!(version(&conn), i64::try_from(MIGRATIONS.len()).unwrap());
        run(&mut conn).unwrap();
        assert_eq!(version(&conn), i64::try_from(MIGRATIONS.len()).unwrap());
    }

    #[test]
    fn a_database_from_a_newer_uniwrt_is_refused() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", 999).unwrap();
        let err = run(&mut conn).unwrap_err();
        assert!(
            matches!(err, Error::TooNew { found: 999, supported } if supported == MIGRATIONS.len()),
            "{err:?}"
        );
    }

    #[test]
    fn a_failed_migration_leaves_the_version_unchanged() {
        let mut conn = Connection::open_in_memory().unwrap();
        // The first migration creates `nodes`; an existing one makes it fail.
        conn.execute_batch("CREATE TABLE nodes (id INTEGER)")
            .unwrap();
        let err = run(&mut conn).unwrap_err();
        assert!(
            matches!(
                err,
                Error::Migration {
                    name: "0001_nodes",
                    ..
                }
            ),
            "{err:?}"
        );
        assert_eq!(version(&conn), 0);
    }
}
