//! What the mirror remembers between starts, in `<root>/.state.json`: each feed's
//! validators and times, why its last refresh failed, the names nodes fetched through the
//! mirror per feed set (kept from then on, D-147), and the last sync. Host directories can't
//! start with a dot, so `.state.json` and `.partial/` (downloads in progress) never meet
//! one.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use uniwrt_core::Timestamp;
use uniwrt_model::{facts::PackageManager, feeds::FeedSet};

use crate::{Error, source::Validators};

pub(crate) const STATE: &str = ".state.json";
pub(crate) const PARTIAL: &str = ".partial";

/// One feed's state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FeedState {
    pub manager: PackageManager,
    #[serde(default)]
    pub validators: Validators,
    /// When upstream was last asked for the index, successfully.
    #[serde(default)]
    pub checked_at: Option<Timestamp>,
    /// When the index kept was written.
    #[serde(default)]
    pub changed_at: Option<Timestamp>,
    /// Why the last refresh failed, until one succeeds.
    #[serde(default)]
    pub error: Option<String>,
}

/// The last sync.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SyncRecord {
    pub at: Timestamp,
    /// What failed, if anything did: feeds that couldn't be refreshed, packages that couldn't
    /// be had.
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct State {
    /// By mirror path.
    #[serde(default)]
    pub feeds: BTreeMap<String, FeedState>,
    /// The names nodes fetched through the mirror, per feed set.
    #[serde(default)]
    pub requested: BTreeMap<String, BTreeSet<String>>,
    #[serde(default)]
    pub synced: Option<SyncRecord>,
}

/// The key a feed set's requested names are kept under.
pub(crate) fn set_key(set: &FeedSet) -> String {
    format!("{}/{}/{}", set.release, set.target, set.arch)
}

pub(crate) fn load(root: &Path) -> State {
    match std::fs::read(root.join(STATE)) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|err| {
            tracing::warn!("the feed mirror's state is unreadable, starting afresh: {err}");
            State::default()
        }),
        Err(_) => State::default(),
    }
}

/// Writes `state` whole (a temporary file renamed over the old one).
pub(crate) async fn save(root: &Path, state: &State) -> Result<(), Error> {
    let bytes = serde_json::to_vec(state).map_err(|err| Error::Storage(err.to_string()))?;
    let temp = root.join(PARTIAL).join("state.json");
    let storage = |err: std::io::Error| Error::Storage(format!("{}: {err}", root.display()));
    tokio::fs::write(&temp, bytes).await.map_err(storage)?;
    tokio::fs::rename(&temp, root.join(STATE))
        .await
        .map_err(storage)
}

/// A feed's directory.
pub(crate) fn feed_dir(root: &Path, mirror_path: &str) -> PathBuf {
    let mut dir = root.to_path_buf();
    dir.extend(mirror_path.split('/'));
    dir
}
