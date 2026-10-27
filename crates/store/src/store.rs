//! The store handle: opening with migrations, reads through the pool, writes through the writer.

use std::{fmt, path::PathBuf, sync::Arc};

use rusqlite::Connection;
use tokio::task;
use uniwrt_core::{Clock, Timestamp};

use crate::{Error, Result, connection, readers::Readers, writer::Writer};

/// Concurrent reads at most: one per core of a Raspberry Pi 4.
const READERS: usize = 4;

/// A handle to the database. Clones share it; repositories are implemented on it.
#[derive(Clone)]
pub struct Store {
    inner: Arc<Inner>,
}

struct Inner {
    path: PathBuf,
    clock: Arc<dyn Clock>,
    writer: Writer,
    readers: Arc<Readers>,
}

impl Store {
    /// Opens the database at `path`, creating it if needed, and applies pending migrations.
    pub async fn open(path: impl Into<PathBuf>, clock: Arc<dyn Clock>) -> Result<Self> {
        let path = path.into();
        let (conn, path) =
            task::spawn_blocking(move || connection::open_writer(&path).map(|conn| (conn, path)))
                .await
                .map_err(|_| Error::Aborted)??;
        let writer = Writer::spawn(conn).map_err(Error::Writer)?;
        let readers = Arc::new(Readers::new(path.clone(), READERS));
        Ok(Self {
            inner: Arc::new(Inner {
                path,
                clock,
                writer,
                readers,
            }),
        })
    }

    /// Commits queued writes, checkpoints and closes the database. Later calls fail with
    /// [`Error::Closed`]; closing again is a no-op.
    pub async fn close(&self) {
        self.inner.readers.close();
        self.inner.writer.close().await;
    }

    pub(crate) fn now(&self) -> Timestamp {
        self.inner.clock.now()
    }

    pub(crate) async fn read<T, F>(&self, read: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        self.inner.readers.run(read).await
    }

    pub(crate) async fn write<T, F>(&self, write: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        self.inner.writer.run(write).await
    }
}

impl fmt::Debug for Store {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Store")
            .field("path", &self.inner.path)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;

    use crate::{Error, testing};

    #[tokio::test]
    async fn creates_a_private_database_in_wal_mode() {
        let (dir, store, _) = testing::open().await;
        let mode = std::fs::metadata(dir.path().join("test.db"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        let (journal, foreign_keys): (String, bool) = store
            .write(|conn| {
                Ok((
                    conn.query_row("PRAGMA journal_mode", [], |row| row.get(0))?,
                    conn.query_row("PRAGMA foreign_keys", [], |row| row.get(0))?,
                ))
            })
            .await
            .unwrap();
        assert_eq!((journal.as_str(), foreign_keys), ("wal", true));
    }

    #[tokio::test]
    async fn bundled_sqlite_has_the_features_we_rely_on() {
        let (_dir, store, _) = testing::open().await;
        let options: Vec<String> = store
            .read(|conn| {
                let mut stmt = conn.prepare("PRAGMA compile_options")?;
                let options = stmt
                    .query_map([], |row| row.get(0))?
                    .collect::<rusqlite::Result<_>>()?;
                Ok(options)
            })
            .await
            .unwrap();
        // FTS5 for log search (M2.5); a thread-safe build for the writer thread and the pool.
        for option in ["ENABLE_FTS5", "THREADSAFE=1"] {
            assert!(
                options.iter().any(|o| o == option),
                "{option} missing from {options:?}"
            );
        }
        assert!(
            rusqlite::version_number() >= 3_045_000,
            "{}",
            rusqlite::version()
        );
    }

    #[tokio::test]
    async fn close_refuses_later_reads_and_writes() {
        let (_dir, store, _) = testing::open().await;
        store.close().await;
        store.close().await;
        assert!(matches!(store.read(|_| Ok(())).await, Err(Error::Closed)));
        assert!(matches!(store.write(|_| Ok(())).await, Err(Error::Closed)));
    }
}
