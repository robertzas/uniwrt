//! The read pool: read-only connections, opened on demand, used on tokio's blocking pool.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex, PoisonError},
};

use rusqlite::Connection;
use tokio::{sync::Semaphore, task};

use crate::{Error, Result, connection};

pub(crate) struct Readers {
    path: PathBuf,
    idle: Mutex<Vec<Connection>>,
    /// One permit per connection, so at most that many exist.
    permits: Arc<Semaphore>,
}

impl Readers {
    pub(crate) fn new(path: PathBuf, max: usize) -> Self {
        Self {
            path,
            idle: Mutex::new(Vec::with_capacity(max)),
            permits: Arc::new(Semaphore::new(max)),
        }
    }

    /// Runs `read` on an idle connection, or a new one when none is idle.
    pub(crate) async fn run<T, F>(self: &Arc<Self>, read: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let permit = Arc::clone(&self.permits)
            .acquire_owned()
            .await
            .map_err(|_| Error::Closed)?;
        let pool = Arc::clone(self);
        task::spawn_blocking(move || {
            let _permit = permit;
            let conn = match pool.lock().pop() {
                Some(conn) => conn,
                None => connection::open_reader(&pool.path)?,
            };
            // A panic drops the connection; the next read opens a replacement.
            let result = read(&conn);
            if !pool.permits.is_closed() {
                pool.lock().push(conn);
            }
            result
        })
        .await
        .map_err(|_| Error::Aborted)?
    }

    /// Refuses later reads and closes the idle connections; reads in flight finish.
    pub(crate) fn close(&self) {
        self.permits.close();
        self.lock().clear();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Connection>> {
        // The list stays consistent even if a holder panicked: push and pop are atomic.
        self.idle.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
