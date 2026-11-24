//! The catalog kept on disk (D-147, M9.0c1b2): every answer ASU or its download server gives a
//! catalog read is written to a directory, and read from there when they can't be reached or
//! fail (5xx), so offers, upgrade plans and the feed mirror's next releases work offline with
//! what was read last. One file per URL, named by its SHA-256; files not written for
//! [`MAX_AGE`] are deleted whenever the releases' overview is written (so never offline).

use std::{
    path::PathBuf,
    time::{Duration, SystemTime},
};

use bytes::Bytes;
use data_encoding::HEXLOWER;

/// How long an answer no read wrote again is kept: a release long gone from the offers.
const MAX_AGE: Duration = Duration::from_hours(24 * 180);

pub(super) struct Kept {
    dir: PathBuf,
}

impl Kept {
    pub(super) fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn path(&self, url: &str) -> PathBuf {
        let hash = ring::digest::digest(&ring::digest::SHA256, url.as_bytes());
        self.dir
            .join(format!("{}.json", HEXLOWER.encode(hash.as_ref())))
    }

    /// Keeps `body`, `url`'s answer; a failure is only logged (the read itself succeeded).
    pub(super) async fn store(&self, url: &str, body: &Bytes, overview: bool) {
        let path = self.path(url);
        let temp = path.with_extension("part");
        let written = async {
            tokio::fs::create_dir_all(&self.dir).await?;
            tokio::fs::write(&temp, body).await?;
            tokio::fs::rename(&temp, &path).await
        }
        .await;
        if let Err(err) = written {
            let _ = tokio::fs::remove_file(&temp).await;
            tracing::warn!("cannot keep {url} in {}: {err}", self.dir.display());
        }
        if overview {
            prune(&self.dir).await;
        }
    }

    /// `url`'s answer as kept last, if it was.
    pub(super) async fn read(&self, url: &str) -> Option<Bytes> {
        tokio::fs::read(self.path(url)).await.ok().map(Bytes::from)
    }
}

/// Deletes the answers not written for [`MAX_AGE`].
async fn prune(dir: &std::path::Path) {
    let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
        return;
    };
    let now = SystemTime::now();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let old = entry
            .metadata()
            .await
            .and_then(|meta| meta.modified())
            .is_ok_and(|at| now.duration_since(at).is_ok_and(|age| age > MAX_AGE));
        if old {
            let _ = tokio::fs::remove_file(entry.path()).await;
        }
    }
}
