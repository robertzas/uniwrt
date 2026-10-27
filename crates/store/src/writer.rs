//! The writer: one thread owns the only write connection and commits queued operations in batches.

use std::{
    io,
    panic::{self, AssertUnwindSafe},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
};

use rusqlite::Connection;
use tokio::sync::{mpsc, oneshot, watch};
use tracing::{trace, warn};

use crate::{Error, Result};

/// Queued operations beyond which callers wait.
const QUEUE: usize = 1_024;
/// The most operations in one transaction, so a batch never holds back its first caller long.
const MAX_BATCH: usize = 256;

type Failure = Arc<rusqlite::Error>;

pub(crate) struct Writer {
    queue: mpsc::Sender<Msg>,
    done: watch::Receiver<bool>,
    /// Committed batches, for tests that check batching.
    #[cfg(test)]
    batches: Arc<AtomicUsize>,
}

enum Msg {
    Op(Box<dyn Op>),
    Close,
}

trait Op: Send {
    /// Runs inside the batch's transaction; false when the operation failed.
    fn run(&mut self, conn: &Connection) -> bool;
    /// Reports the result: committed when there is no `failure`, which is the batch's.
    fn complete(self: Box<Self>, failure: Option<&Failure>);
}

struct Pending<F, T> {
    work: Option<F>,
    result: Option<Result<T>>,
    reply: oneshot::Sender<Result<T>>,
}

impl<F, T> Op for Pending<F, T>
where
    F: FnOnce(&Connection) -> Result<T> + Send,
    T: Send,
{
    fn run(&mut self, conn: &Connection) -> bool {
        let Some(work) = self.work.take() else {
            return false;
        };
        let result = work(conn);
        let ok = result.is_ok();
        self.result = Some(result);
        ok
    }

    fn complete(self: Box<Self>, failure: Option<&Failure>) {
        let result = match (self.result, failure) {
            (Some(Err(err)), _) => Err(err),
            (Some(Ok(value)), None) => Ok(value),
            (_, Some(failure)) => Err(Error::Commit(Arc::clone(failure))),
            // It panicked.
            (None, None) => Err(Error::Aborted),
        };
        // The caller may have stopped waiting; the write stands either way.
        let _ = self.reply.send(result);
    }
}

impl Writer {
    pub(crate) fn spawn(conn: Connection) -> io::Result<Self> {
        let (queue, inbox) = mpsc::channel(QUEUE);
        let (finished, done) = watch::channel(false);
        let batches = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&batches);
        thread::Builder::new()
            .name("uniwrt-db-writer".to_owned())
            .spawn(move || {
                work(conn, inbox, &counter);
                finished.send_replace(true);
            })?;
        Ok(Self {
            queue,
            done,
            #[cfg(test)]
            batches,
        })
    }

    /// Runs `work` in the next batch and returns its result once the batch has committed.
    pub(crate) async fn run<T, F>(&self, work: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let (reply, result) = oneshot::channel();
        let op = Box::new(Pending {
            work: Some(work),
            result: None,
            reply,
        });
        self.queue
            .send(Msg::Op(op))
            .await
            .map_err(|_| Error::Closed)?;
        result.await.map_err(|_| Error::Aborted)?
    }

    /// Commits what is queued, refuses later writes, and waits until the connection is closed.
    pub(crate) async fn close(&self) {
        // Fails only when the writer is already closing.
        let _ = self.queue.send(Msg::Close).await;
        let mut done = self.done.clone();
        // An error means the thread is gone, which is just as final.
        let _ = done.wait_for(|&done| done).await;
    }

    #[cfg(test)]
    pub(crate) fn batches(&self) -> usize {
        self.batches.load(Ordering::Relaxed)
    }
}

fn work(conn: Connection, mut inbox: mpsc::Receiver<Msg>, batches: &AtomicUsize) {
    let mut batch: Vec<Box<dyn Op>> = Vec::with_capacity(MAX_BATCH);
    while let Some(first) = inbox.blocking_recv() {
        let mut next = Some(first);
        while let Some(msg) = next {
            match msg {
                Msg::Op(op) => batch.push(op),
                // Buffered messages are still received; new ones are refused.
                Msg::Close => inbox.close(),
            }
            next = if batch.len() < MAX_BATCH {
                inbox.try_recv().ok()
            } else {
                None
            };
        }
        if !batch.is_empty() {
            commit(&conn, &mut batch);
            batches.fetch_add(1, Ordering::Relaxed);
        }
    }
    shut_down(conn);
}

fn commit(conn: &Connection, batch: &mut Vec<Box<dyn Op>>) {
    let ops = batch.len();
    let failure = match run_batch(conn, batch) {
        Ok(()) => {
            trace!(ops, "committed a write batch");
            None
        }
        Err(err) => {
            warn!(ops, error = %err, "a write batch did not commit");
            Some(Arc::new(err))
        }
    };
    for op in batch.drain(..) {
        op.complete(failure.as_ref());
    }
}

fn run_batch(conn: &Connection, batch: &mut [Box<dyn Op>]) -> rusqlite::Result<()> {
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result = batch
        .iter_mut()
        .try_for_each(|op| {
            conn.execute_batch("SAVEPOINT op")?;
            // A panicking operation is a bug, but it must not take down the writer and with it
            // every later write.
            let ok = panic::catch_unwind(AssertUnwindSafe(|| op.run(conn))).unwrap_or(false);
            if !ok {
                conn.execute_batch("ROLLBACK TO op")?;
            }
            conn.execute_batch("RELEASE op")
        })
        .and_then(|()| conn.execute_batch("COMMIT"));
    if result.is_err() && !conn.is_autocommit() {
        // SQLite may have rolled back already; if this fails too, the next BEGIN reports it.
        let _ = conn.execute_batch("ROLLBACK");
    }
    result
}

/// Updates query statistics and folds the WAL into the database before closing.
fn shut_down(conn: Connection) {
    let result = conn
        .execute_batch("PRAGMA optimize; PRAGMA wal_checkpoint(TRUNCATE);")
        .map_err(|err| ("finish", err))
        .and_then(|()| conn.close().map_err(|(_, err)| ("close", err)));
    if let Err((step, err)) = result {
        warn!(error = %err, "cannot {step} the database cleanly");
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::{Path, PathBuf},
        sync::{Arc, mpsc as std_mpsc},
    };

    use rusqlite::Connection;
    use tempfile::TempDir;
    use tokio::task::JoinSet;

    use super::{MAX_BATCH, Writer};
    use crate::Error;

    fn writer() -> (TempDir, Writer, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("PRAGMA journal_mode = wal; CREATE TABLE t (v INTEGER NOT NULL UNIQUE)")
            .unwrap();
        (dir, Writer::spawn(conn).unwrap(), path)
    }

    fn values(path: &Path) -> Vec<i64> {
        let conn = Connection::open(path).unwrap();
        let mut stmt = conn.prepare("SELECT v FROM t ORDER BY v").unwrap();
        stmt.query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    fn insert(v: i64) -> impl FnOnce(&Connection) -> crate::Result<()> + Send + 'static {
        move |conn| {
            conn.execute("INSERT INTO t (v) VALUES (?1)", [v])?;
            Ok(())
        }
    }

    /// Holds the writer inside a batch until the returned sender fires, so that the operations
    /// queued meanwhile land in the next batch together.
    async fn hold(
        writer: &Arc<Writer>,
    ) -> (
        std_mpsc::Sender<()>,
        tokio::task::JoinHandle<crate::Result<()>>,
    ) {
        let (release, held) = std_mpsc::channel::<()>();
        let (entered_tx, entered) = tokio::sync::oneshot::channel();
        let writer = Arc::clone(writer);
        let task = tokio::spawn(async move {
            writer
                .run(move |_| {
                    let _ = entered_tx.send(());
                    let _ = held.recv();
                    Ok(())
                })
                .await
        });
        entered.await.unwrap();
        (release, task)
    }

    #[tokio::test]
    async fn queued_operations_share_one_batch_and_fail_alone() {
        let (_dir, writer, path) = writer();
        let writer = Arc::new(writer);
        let (release, held) = hold(&writer).await;
        let mut ops = JoinSet::new();
        for v in [1, 2, 2, 3] {
            let writer = Arc::clone(&writer);
            ops.spawn(async move { (v, writer.run(insert(v)).await) });
        }
        // One operation inserts and then fails: its savepoint undoes the insert.
        let failing = {
            let writer = Arc::clone(&writer);
            tokio::spawn(async move {
                writer
                    .run(|conn| {
                        conn.execute("INSERT INTO t (v) VALUES (100)", [])?;
                        Err::<(), _>(Error::Closed)
                    })
                    .await
            })
        };
        while writer.queue.capacity() > super::QUEUE - 5 {
            tokio::task::yield_now().await;
        }
        release.send(()).unwrap();
        held.await.unwrap().unwrap();
        let mut results = ops.join_all().await;
        results.sort_by_key(|(v, result)| (*v, result.is_err()));
        assert!(
            matches!(
                results[..],
                [
                    (1, Ok(())),
                    (2, Ok(())),
                    (2, Err(Error::Sqlite(_))),
                    (3, Ok(()))
                ]
            ),
            "{results:?}"
        );
        assert!(matches!(failing.await.unwrap(), Err(Error::Closed)));
        assert_eq!(values(&path), [1, 2, 3]);
        assert_eq!(writer.batches(), 2);
    }

    #[tokio::test]
    async fn a_panicking_operation_is_aborted_and_the_writer_keeps_going() {
        let (_dir, writer, path) = writer();
        let result = writer
            .run(|conn| -> crate::Result<()> {
                conn.execute("INSERT INTO t (v) VALUES (7)", [])?;
                panic!("bug in a write operation");
            })
            .await;
        assert!(matches!(result, Err(Error::Aborted)), "{result:?}");
        writer.run(insert(8)).await.unwrap();
        assert_eq!(values(&path), [8]);
    }

    #[tokio::test]
    async fn a_large_queue_is_split_into_bounded_batches() {
        let (_dir, writer, path) = writer();
        let writer = Arc::new(writer);
        let (release, held) = hold(&writer).await;
        let mut ops = JoinSet::new();
        let count = i64::try_from(MAX_BATCH).unwrap() + 10;
        for v in 0..count {
            let writer = Arc::clone(&writer);
            ops.spawn(async move { writer.run(insert(v)).await });
        }
        while writer.queue.capacity() > super::QUEUE - usize::try_from(count).unwrap() {
            tokio::task::yield_now().await;
        }
        release.send(()).unwrap();
        held.await.unwrap().unwrap();
        assert!(ops.join_all().await.iter().all(Result::is_ok));
        assert_eq!(values(&path).len(), usize::try_from(count).unwrap());
        assert_eq!(writer.batches(), 3);
    }

    #[tokio::test]
    async fn close_commits_what_is_queued_and_refuses_later_writes() {
        let (_dir, writer, path) = writer();
        let writer = Arc::new(writer);
        let (release, held) = hold(&writer).await;
        let queued = {
            let writer = Arc::clone(&writer);
            tokio::spawn(async move { writer.run(insert(1)).await })
        };
        while writer.queue.capacity() == super::QUEUE {
            tokio::task::yield_now().await;
        }
        let closing = {
            let writer = Arc::clone(&writer);
            tokio::spawn(async move { writer.close().await })
        };
        while writer.queue.capacity() > super::QUEUE - 2 {
            tokio::task::yield_now().await;
        }
        release.send(()).unwrap();
        closing.await.unwrap();
        held.await.unwrap().unwrap();
        queued.await.unwrap().unwrap();
        assert!(matches!(writer.run(insert(2)).await, Err(Error::Closed)));
        writer.close().await;
        assert_eq!(values(&path), [1]);
    }
}
