//! Opening connections: the private database file, the writer's pragmas and migrations, readers.

use std::{fs::OpenOptions, io, os::unix::fs::OpenOptionsExt as _, path::Path, time::Duration};

use rusqlite::{Connection, OpenFlags};

use crate::{Error, Result, migrate};

/// How long a connection waits for a lock: only checkpoints and external tools ever hold one.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Opens the one write connection: creates the file if needed, enables WAL and migrates.
pub(crate) fn open_writer(path: &Path) -> Result<Connection> {
    create_private(path).map_err(|source| Error::Create {
        path: path.to_owned(),
        source,
    })?;
    let open_error = |source| Error::Open {
        path: path.to_owned(),
        source,
    };
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let mut conn = Connection::open_with_flags(path, flags).map_err(open_error)?;
    let mode: String = conn
        .pragma_update_and_check(None, "journal_mode", "wal", |row| row.get(0))
        .map_err(open_error)?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(Error::NoWal {
            path: path.to_owned(),
            mode,
        });
    }
    conn.pragma_update(None, "synchronous", "NORMAL")
        .and_then(|()| conn.pragma_update(None, "foreign_keys", true))
        .and_then(|()| conn.busy_timeout(BUSY_TIMEOUT))
        .map_err(open_error)?;
    migrate::run(&mut conn)?;
    // Recommended for long-lived connections; cheap when there is nothing to analyze.
    conn.execute_batch("PRAGMA optimize = 0x10002")
        .map_err(open_error)?;
    Ok(conn)
}

/// Opens a read-only connection to a database that `open_writer` has prepared.
pub(crate) fn open_reader(path: &Path) -> Result<Connection> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let open_error = |source| Error::Open {
        path: path.to_owned(),
        source,
    };
    let conn = Connection::open_with_flags(path, flags).map_err(open_error)?;
    conn.busy_timeout(BUSY_TIMEOUT).map_err(open_error)?;
    Ok(conn)
}

/// Creates the database file with mode 0600 unless it exists; SQLite creates its WAL and
/// shared-memory files with the same mode.
fn create_private(path: &Path) -> io::Result<()> {
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
    {
        Err(err) if err.kind() != io::ErrorKind::AlreadyExists => Err(err),
        _ => Ok(()),
    }
}
