//! Serving the mirror to nodes (D-147): a planned feed's index (refreshed first when upstream
//! was last asked over an hour ago and answers within 30 s; else the copy kept), or a package
//! its index lists: kept, or downloaded now, checked, kept and remembered as wanted (pull
//! through). Nothing else is served, so the mirror is no proxy to anywhere else.

use std::time::Duration;

use crate::{
    feed::valid_segment,
    index,
    mirror::Mirror,
    state,
    sync::{fetch_package, refresh_index},
};

/// How long an index is served without asking upstream again.
const FRESH: Duration = Duration::from_hours(1);
/// How long a node's request waits for upstream's index.
const ASK: Duration = Duration::from_secs(30);

/// A file to send: open, so a refresh or sweep after this doesn't change what is sent.
#[derive(Debug)]
pub struct Served {
    pub file: tokio::fs::File,
    pub len: u64,
}

/// Why a request isn't served.
#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    #[error("not in the feed mirror")]
    NotFound,
    #[error("{0}")]
    Unavailable(String),
}

impl Mirror {
    /// The file at `path` (below `/feeds/`: a planned feed's mirror path, then a file).
    pub async fn serve(&self, path: &str) -> Result<Served, ServeError> {
        let inner = &self.inner;
        let (dir, file) = path.rsplit_once('/').ok_or(ServeError::NotFound)?;
        if !valid_segment(file) {
            return Err(ServeError::NotFound);
        }
        let planned = inner.planned();
        let planned = planned.get(dir).ok_or(ServeError::NotFound)?;
        let manager = planned.set.manager;
        let files = index::files(manager);
        let dest = state::feed_dir(&inner.root, dir).join(file);
        if files.contains(&file) {
            if file == files[0] {
                let checked = inner
                    .state()
                    .feeds
                    .get(dir)
                    .and_then(|feed| feed.checked_at);
                let now = inner.clock.now();
                let stale = checked.is_none_or(|at| now.saturating_duration_since(at) > FRESH);
                if stale {
                    let _ = tokio::time::timeout(ASK, refresh_index(inner, &planned.feed, manager))
                        .await;
                }
            }
            let guard = inner.guard(format!("feed:{dir}"));
            let _held = guard.lock().await;
            return open(&dest).await;
        }
        if let Ok(served) = open(&dest).await {
            return Ok(served);
        }
        let index = inner
            .index(dir, manager)
            .await
            .ok_or_else(|| ServeError::Unavailable("the feed's index isn't kept yet".to_owned()))?;
        let entry = index
            .entries
            .iter()
            .find(|entry| entry.file == file)
            .ok_or(ServeError::NotFound)?;
        let key = state::set_key(&planned.set);
        let new = inner.with_state(|state| {
            state
                .requested
                .entry(key)
                .or_default()
                .insert(entry.name.clone())
        });
        if new {
            let _ = state::save(&inner.root, &inner.state()).await;
        }
        fetch_package(inner, &planned.feed, entry)
            .await
            .map_err(ServeError::Unavailable)?;
        open(&dest).await
    }
}

async fn open(path: &std::path::Path) -> Result<Served, ServeError> {
    let file = tokio::fs::File::open(path)
        .await
        .map_err(|_| ServeError::NotFound)?;
    let len = file
        .metadata()
        .await
        .map_err(|err| ServeError::Unavailable(err.to_string()))?
        .len();
    Ok(Served { file, len })
}
