//! [`Mirror`]: the feeds kept in a directory of the controller's, what is planned for them
//! ([`Plan`]), and what they hold ([`Mirror::status`]). The sync is in [`crate::sync`], the
//! serving in [`crate::serve`].

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use uniwrt_core::{Clock, Timestamp};
use uniwrt_model::{
    facts::PackageManager,
    feeds::{FeedSet, FeedStatus, ImageStatus, MirrorStatus, SetStatus},
    firmware::FirmwareImage,
};

use crate::{
    Error,
    feed::Feed,
    index::{self, Index},
    source::Source,
    state::{self, PARTIAL, State},
};

/// What to keep (D-147).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub sets: Vec<SetPlan>,
    /// Every package of every feed (the *Keep whole feeds* setting).
    pub whole: bool,
    /// OpenWrt's own images for the upgrades on offer (M9.0c1b2).
    pub images: Vec<FirmwareImage>,
}

/// One feed set to keep: its feeds and the names wanted of them (the packages uniwrt can
/// install and those installed on its nodes); the mirror adds what nodes fetched through it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetPlan {
    pub set: FeedSet,
    pub feeds: Vec<Feed>,
    pub wanted: BTreeSet<String>,
    /// Kept for an upgrade on offer: no node runs it yet.
    pub offered: bool,
}

/// A planned feed, as serving looks it up by its mirror path.
#[derive(Debug, Clone)]
pub(crate) struct Planned {
    pub feed: Feed,
    pub set: FeedSet,
}

/// What the last sync found of a feed set.
#[derive(Debug, Clone, Default)]
pub(crate) struct SetReport {
    pub missing: Vec<String>,
    pub failed: Vec<String>,
}

/// What is kept of a feed: packages and their bytes.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Kept {
    pub files: u32,
    pub bytes: u64,
}

/// An index read, with when it was written.
pub(crate) type ReadIndex = (Option<Timestamp>, Arc<Index>);

pub(crate) struct Inner {
    pub root: PathBuf,
    pub source: Arc<dyn Source>,
    pub clock: Arc<dyn Clock>,
    pub state: Mutex<State>,
    pub plan: Mutex<Arc<Plan>>,
    /// The planned feeds by mirror path.
    pub planned: Mutex<Arc<HashMap<String, Planned>>>,
    /// The indexes read, by mirror path, with when they were written.
    pub indexes: Mutex<HashMap<String, ReadIndex>>,
    /// One lock per feed (its index) and per package file being downloaded.
    pub locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    pub sync: tokio::sync::Mutex<()>,
    pub syncing: AtomicBool,
    pub reports: Mutex<BTreeMap<FeedSet, SetReport>>,
    pub kept: Mutex<HashMap<String, Kept>>,
    /// Why the last sync couldn't keep an image, by its URL.
    pub image_errors: Mutex<BTreeMap<String, String>>,
    pub partial: AtomicU64,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Inner {
    pub(crate) fn guard(&self, key: String) -> Arc<tokio::sync::Mutex<()>> {
        lock(&self.locks).entry(key).or_default().clone()
    }

    /// A fresh path for a download in progress.
    pub(crate) fn partial(&self) -> PathBuf {
        let n = self.partial.fetch_add(1, Ordering::Relaxed);
        self.root.join(PARTIAL).join(n.to_string())
    }

    pub(crate) fn with_state<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        f(&mut lock(&self.state))
    }

    pub(crate) fn state(&self) -> State {
        lock(&self.state).clone()
    }

    pub(crate) fn plan(&self) -> Arc<Plan> {
        lock(&self.plan).clone()
    }

    pub(crate) fn set_image_errors(&self, errors: BTreeMap<String, String>) {
        *lock(&self.image_errors) = errors;
    }

    pub(crate) fn planned(&self) -> Arc<HashMap<String, Planned>> {
        lock(&self.planned).clone()
    }

    pub(crate) fn set_plan(&self, plan: Plan) {
        let mut planned = HashMap::new();
        for set in &plan.sets {
            for feed in &set.feeds {
                planned.insert(
                    feed.mirror_path(),
                    Planned {
                        feed: feed.clone(),
                        set: set.set.clone(),
                    },
                );
            }
        }
        *lock(&self.planned) = Arc::new(planned);
        *lock(&self.plan) = Arc::new(plan);
    }

    pub(crate) fn set_reports(&self, reports: BTreeMap<FeedSet, SetReport>) {
        *lock(&self.reports) = reports;
    }

    pub(crate) fn set_kept(&self, kept: HashMap<String, Kept>) {
        *lock(&self.kept) = kept;
    }

    pub(crate) fn add_kept(&self, mirror_path: &str, bytes: u64) {
        let mut kept = lock(&self.kept);
        let entry = kept.entry(mirror_path.to_owned()).or_default();
        entry.files += 1;
        entry.bytes += bytes;
    }

    /// The feed's index, read from disk when it changed since it was last read.
    pub(crate) async fn index(
        &self,
        mirror_path: &str,
        manager: PackageManager,
    ) -> Option<Arc<Index>> {
        let changed = lock(&self.state)
            .feeds
            .get(mirror_path)
            .and_then(|feed| feed.changed_at);
        if let Some((at, index)) = lock(&self.indexes).get(mirror_path)
            && *at == changed
        {
            return Some(index.clone());
        }
        let path = state::feed_dir(&self.root, mirror_path).join(index::files(manager)[0]);
        let read = tokio::task::spawn_blocking(move || {
            std::fs::read(&path)
                .ok()
                .and_then(|bytes| index::parse(manager, &bytes).ok())
        })
        .await
        .ok()
        .flatten()?;
        let read = Arc::new(read);
        lock(&self.indexes).insert(mirror_path.to_owned(), (changed, read.clone()));
        Some(read)
    }

    pub(crate) fn remember(&self, mirror_path: &str, changed: Option<Timestamp>, index: Index) {
        lock(&self.indexes).insert(mirror_path.to_owned(), (changed, Arc::new(index)));
    }
}

/// The feeds kept in a directory of the controller's.
#[derive(Clone)]
pub struct Mirror {
    pub(crate) inner: Arc<Inner>,
}

impl Mirror {
    /// Opens the mirror in `root` (made if missing; downloads a previous run left unfinished
    /// are removed). Nothing is served until a [`Plan`] is given ([`Mirror::sync`]).
    pub async fn open(
        root: PathBuf,
        source: Arc<dyn Source>,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, Error> {
        let partial = root.join(PARTIAL);
        let storage = |err: std::io::Error| Error::Storage(format!("{}: {err}", root.display()));
        let _ = tokio::fs::remove_dir_all(&partial).await;
        tokio::fs::create_dir_all(&partial).await.map_err(storage)?;
        let state = state::load(&root);
        Ok(Self {
            inner: Arc::new(Inner {
                root,
                source,
                clock,
                state: Mutex::new(state),
                plan: Mutex::default(),
                planned: Mutex::default(),
                indexes: Mutex::default(),
                locks: Mutex::default(),
                sync: tokio::sync::Mutex::new(()),
                syncing: AtomicBool::new(false),
                reports: Mutex::default(),
                kept: Mutex::default(),
                image_errors: Mutex::default(),
                partial: AtomicU64::new(0),
            }),
        })
    }

    pub fn root(&self) -> &Path {
        &self.inner.root
    }

    /// What the mirror holds of the plan last given.
    pub fn status(&self) -> MirrorStatus {
        let inner = &self.inner;
        let plan = lock(&inner.plan).clone();
        let state = inner.state();
        let reports = lock(&inner.reports).clone();
        let kept = lock(&inner.kept).clone();
        let indexes = lock(&inner.indexes).clone();
        let mut total = 0;
        let sets = plan
            .sets
            .iter()
            .map(|set| {
                let feeds: Vec<FeedStatus> = set
                    .feeds
                    .iter()
                    .map(|feed| {
                        let path = feed.mirror_path();
                        let own = state.feeds.get(&path);
                        let held = kept.get(&path).copied().unwrap_or_default();
                        FeedStatus {
                            url: feed.url(),
                            index_at: own.and_then(|feed| feed.changed_at),
                            checked_at: own.and_then(|feed| feed.checked_at),
                            error: own.and_then(|feed| feed.error.clone()),
                            listed: indexes.get(&path).map_or(0, |(_, index)| {
                                u32::try_from(index.entries.len()).unwrap_or(u32::MAX)
                            }),
                            kept: held.files,
                            bytes: held.bytes,
                        }
                    })
                    .collect();
                let report = reports.get(&set.set).cloned().unwrap_or_default();
                let bytes = feeds.iter().map(|feed| feed.bytes).sum();
                total += bytes;
                SetStatus {
                    set: set.set.clone(),
                    kept: feeds.iter().map(|feed| feed.kept).sum(),
                    bytes,
                    feeds,
                    missing: report.missing,
                    failed: report.failed,
                    offered: set.offered,
                }
            })
            .collect();
        let errors = lock(&inner.image_errors).clone();
        let images = plan
            .images
            .iter()
            .map(|image| {
                let kept = self.image(image).is_some();
                if kept {
                    total += image.size_bytes.unwrap_or(0);
                }
                ImageStatus {
                    url: image.url.clone(),
                    name: image.name.clone(),
                    bytes: image.size_bytes,
                    kept,
                    error: errors.get(&image.url).cloned(),
                }
            })
            .collect();
        MirrorStatus {
            sets,
            images,
            bytes: total,
            synced_at: state.synced.as_ref().map(|sync| sync.at),
            sync_error: state.synced.and_then(|sync| sync.error),
            syncing: inner.syncing.load(Ordering::Relaxed),
        }
    }
}
