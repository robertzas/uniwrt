//! Test support: a fresh store in a temporary directory, on a manual clock.

use std::sync::Arc;

use tempfile::TempDir;
use uniwrt_core::{ManualClock, Timestamp};

use crate::Store;

/// The manual clock's start: 2026-10-27T00:00:00Z.
pub(crate) const START: Timestamp = Timestamp::from_unix_secs(1_793_059_200);

/// Opens `test.db` in a new temporary directory, which is deleted when the guard drops.
pub(crate) async fn open() -> (TempDir, Store, ManualClock) {
    let dir = tempfile::tempdir().unwrap();
    let clock = ManualClock::new(START);
    let store = Store::open(dir.path().join("test.db"), Arc::new(clock.clone()))
        .await
        .unwrap();
    (dir, store, clock)
}
