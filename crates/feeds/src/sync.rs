//! Bringing the mirror to a [`Plan`] (D-147): each feed's index refreshed (conditionally;
//! a failure keeps the copy kept and says why), then the packages each feed set needs
//! downloaded (the wanted names and those nodes fetched before, with their dependencies, or
//! every package with *Keep whole feeds*), each checked against its index before it is kept,
//! then every file no planned feed's index needs deleted: older versions, packages nothing
//! wants any more, the feeds of sets no longer planned.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, atomic::Ordering},
    time::SystemTime,
};

use tokio::{sync::Semaphore, task::JoinSet};
use uniwrt_model::facts::PackageManager;

use crate::{
    Error, closure,
    feed::Feed,
    index::{self, Entry, MAX_INDEX},
    mirror::{Inner, Kept, Mirror, Plan, SetPlan, SetReport},
    source::{Fetched, Validators},
    state::{self, FeedState, PARTIAL, STATE, SyncRecord},
    verify,
};

/// How many packages download at once.
const PARALLEL: usize = 4;
/// The largest package kept.
pub(crate) const MAX_PACKAGE: u64 = 1 << 30;
/// How many failures a set's report lists.
const MAX_FAILED: usize = 20;

impl Mirror {
    /// Brings the mirror to `plan`. With `refresh`, every index is asked for again
    /// (conditionally); without, only feeds that have none yet are. One sync runs at a time;
    /// the plan serves at once. Fails only when the mirror's directory can't be written.
    pub async fn sync(&self, plan: Plan, refresh: bool) -> Result<(), Error> {
        let inner = &self.inner;
        let _one = inner.sync.lock().await;
        inner.syncing.store(true, Ordering::Relaxed);
        let started = SystemTime::now();
        inner.set_plan(plan.clone());
        let result = run(inner, &plan, refresh, started).await;
        inner.syncing.store(false, Ordering::Relaxed);
        result
    }
}

async fn run(
    inner: &Arc<Inner>,
    plan: &Plan,
    refresh: bool,
    started: SystemTime,
) -> Result<(), Error> {
    let mut problems = Vec::new();
    let mut keep: HashSet<PathBuf> = HashSet::new();
    let mut reports = BTreeMap::new();
    let mut requested = inner.state().requested;
    let planned_keys: BTreeSet<String> = plan
        .sets
        .iter()
        .map(|set| state::set_key(&set.set))
        .collect();
    requested.retain(|key, _| planned_keys.contains(key));
    inner.with_state(|state| state.requested.clone_from(&requested));

    for set in &plan.sets {
        let report = sync_set(
            inner,
            set,
            plan.whole,
            refresh,
            &requested,
            &mut keep,
            &mut problems,
        )
        .await;
        reports.insert(set.set.clone(), report);
    }

    let root = inner.root.clone();
    let planned: Vec<String> = plan
        .sets
        .iter()
        .flat_map(|set| set.feeds.iter().map(Feed::mirror_path))
        .collect();
    let kept = tokio::task::spawn_blocking(move || sweep(&root, &keep, &planned, started))
        .await
        .map_err(|err| Error::Storage(err.to_string()))?;
    inner.set_kept(kept);
    inner.set_reports(reports);
    let at = inner.clock.now();
    let error = (!problems.is_empty()).then(|| problems.join("; "));
    inner.with_state(|state| {
        let planned: HashSet<String> = plan
            .sets
            .iter()
            .flat_map(|set| set.feeds.iter().map(Feed::mirror_path))
            .collect();
        state.feeds.retain(|path, _| planned.contains(path));
        state.synced = Some(SyncRecord { at, error });
    });
    state::save(&inner.root, &inner.state()).await
}

/// One feed set's part of a sync: its indexes (asked for again with `refresh`), then its
/// packages; what to keep goes in `keep`.
async fn sync_set(
    inner: &Arc<Inner>,
    set: &SetPlan,
    whole: bool,
    refresh: bool,
    requested: &BTreeMap<String, BTreeSet<String>>,
    keep: &mut HashSet<PathBuf>,
    problems: &mut Vec<String>,
) -> SetReport {
    let manager = set.set.manager;
    let mut indexes = Vec::new();
    for feed in &set.feeds {
        let path = feed.mirror_path();
        let dir = state::feed_dir(&inner.root, &path);
        for file in index::files(manager) {
            keep.insert(dir.join(file));
        }
        let has = dir.join(index::files(manager)[0]).exists();
        if (refresh || !has)
            && let Err(why) = refresh_index(inner, feed, manager).await
        {
            problems.push(format!("{}: {why}", feed.url()));
        }
        if let Some(index) = inner.index(&path, manager).await {
            indexes.push((feed.clone(), index));
        }
    }
    let refs: Vec<&index::Index> = indexes.iter().map(|(_, index)| index.as_ref()).collect();
    let resolved = if whole {
        closure::everything(&refs)
    } else {
        let fetched = requested.get(&state::set_key(&set.set));
        let names = set
            .wanted
            .iter()
            .chain(fetched.into_iter().flatten())
            .map(String::as_str);
        closure::resolve(&refs, names)
    };
    let mut report = SetReport {
        missing: resolved.missing.into_iter().collect(),
        failed: Vec::new(),
    };
    let semaphore = Arc::new(Semaphore::new(PARALLEL));
    let mut tasks = JoinSet::new();
    for &(i, j) in &resolved.files {
        let (feed, index) = &indexes[i];
        let entry = index.entries[j].clone();
        let dir = state::feed_dir(&inner.root, &feed.mirror_path());
        keep.insert(dir.join(&entry.file));
        if dir.join(&entry.file).exists() {
            continue;
        }
        let (inner, feed, semaphore) = (inner.clone(), feed.clone(), semaphore.clone());
        tasks.spawn(async move {
            let _permit = semaphore.acquire().await;
            fetch_package(&inner, &feed, &entry).await.err()
        });
    }
    while let Some(done) = tasks.join_next().await {
        if let Ok(Some(why)) = done {
            report.failed.push(why);
        }
    }
    report.failed.sort();
    if !report.failed.is_empty() {
        problems.push(format!(
            "{} packages of {} {} couldn't be had",
            report.failed.len(),
            set.set.release,
            set.set.target
        ));
        report.failed.truncate(MAX_FAILED);
    }
    report
}

/// Asks upstream for `feed`'s index, keeping the new one once it reads (opkg's signature
/// with it). Under the feed's lock, so serving never sees half of a refresh.
pub(crate) async fn refresh_index(
    inner: &Inner,
    feed: &Feed,
    manager: PackageManager,
) -> Result<(), String> {
    let path = feed.mirror_path();
    let guard = inner.guard(format!("feed:{path}"));
    let _held = guard.lock().await;
    let dir = state::feed_dir(&inner.root, &path);
    let files = index::files(manager);
    let has = dir.join(files[0]).exists();
    let validators = if has {
        inner
            .state()
            .feeds
            .get(&path)
            .map_or_else(Validators::default, |feed| feed.validators.clone())
    } else {
        Validators::default()
    };
    let result = refresh_locked(inner, feed, manager, &dir, &validators).await;
    let now = inner.clock.now();
    inner.with_state(|state| {
        let entry = state.feeds.entry(path.clone()).or_insert(FeedState {
            manager,
            validators: Validators::default(),
            checked_at: None,
            changed_at: None,
            error: None,
        });
        entry.manager = manager;
        match &result {
            Ok(Some((validators, _))) => {
                entry.validators = validators.clone();
                entry.checked_at = Some(now);
                entry.changed_at = Some(now);
                entry.error = None;
            }
            Ok(None) => {
                entry.checked_at = Some(now);
                entry.error = None;
            }
            Err(why) => entry.error = Some(why.clone()),
        }
    });
    match result {
        Ok(Some((_, index))) => {
            inner.remember(&path, Some(now), index);
            Ok(())
        }
        Ok(None) => Ok(()),
        Err(why) => Err(why),
    }
}

async fn refresh_locked(
    inner: &Inner,
    feed: &Feed,
    manager: PackageManager,
    dir: &Path,
    validators: &Validators,
) -> Result<Option<(Validators, index::Index)>, String> {
    let files = index::files(manager);
    tokio::fs::create_dir_all(dir)
        .await
        .map_err(|err| format!("{}: {err}", dir.display()))?;
    let mut written = Vec::new();
    let mut fresh = None;
    for (n, file) in files.iter().enumerate() {
        let temp = inner.partial();
        // Only the index proper is asked conditionally: its signature follows it.
        let ask = if n == 0 {
            validators.clone()
        } else {
            Validators::default()
        };
        let fetched = inner
            .source
            .fetch(&feed.file_url(file), &ask, &temp, MAX_INDEX)
            .await;
        match fetched {
            Ok(Fetched::NotModified) if n == 0 => return Ok(None),
            Ok(Fetched::Written { validators, .. }) => {
                if n == 0 {
                    let bytes = tokio::fs::read(&temp)
                        .await
                        .map_err(|err| err.to_string())?;
                    let index = tokio::task::spawn_blocking(move || index::parse(manager, &bytes))
                        .await
                        .map_err(|err| err.to_string())?
                        .map_err(|err| format!("{file}: {err}"))?;
                    fresh = Some((validators, index));
                }
                written.push((temp, dir.join(file)));
            }
            Ok(Fetched::NotModified) => {
                return Err(format!("{file} answered not modified to a plain request"));
            }
            Err(err) => {
                for (temp, _) in &written {
                    let _ = tokio::fs::remove_file(temp).await;
                }
                let _ = tokio::fs::remove_file(&temp).await;
                return Err(err.to_string());
            }
        }
    }
    // The signature first: an index is never served beside an older signature.
    for (temp, dest) in written.iter().rev() {
        tokio::fs::rename(temp, dest)
            .await
            .map_err(|err| format!("{}: {err}", dest.display()))?;
    }
    Ok(fresh)
}

/// Downloads `entry` of `feed` and keeps it once it matches its index. Under the file's lock,
/// so a package is downloaded once however many ask for it.
pub(crate) async fn fetch_package(inner: &Inner, feed: &Feed, entry: &Entry) -> Result<(), String> {
    let path = feed.mirror_path();
    let dest = state::feed_dir(&inner.root, &path).join(&entry.file);
    let guard = inner.guard(format!("file:{}", dest.display()));
    let _held = guard.lock().await;
    if dest.exists() {
        return Ok(());
    }
    let temp = inner.partial();
    let url = feed.file_url(&entry.file);
    let max = entry.size.min(MAX_PACKAGE);
    let result = async {
        match inner
            .source
            .fetch(&url, &Validators::default(), &temp, max)
            .await
        {
            Ok(Fetched::Written { .. }) => {}
            Ok(Fetched::NotModified) => {
                return Err(format!("{url}: not modified to a plain request"));
            }
            Err(err) => return Err(err.to_string()),
        }
        let (check, entry_copy, temp_copy) = (verify::check, entry.clone(), temp.clone());
        tokio::task::spawn_blocking(move || check(&entry_copy, &temp_copy))
            .await
            .map_err(|err| err.to_string())??;
        tokio::fs::rename(&temp, &dest)
            .await
            .map_err(|err| format!("{}: {err}", dest.display()))
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&temp).await;
    } else {
        inner.add_kept(&path, entry.size);
    }
    result.map_err(|why| format!("{} {}: {why}", entry.name, entry.version))
}

/// Deletes every file under `root` that isn't in `keep` (but the mirror's own and files
/// written since `started`, which serving may just have fetched), then empty directories;
/// returns what is kept of each planned feed.
fn sweep(
    root: &Path,
    keep: &HashSet<PathBuf>,
    planned: &[String],
    started: SystemTime,
) -> HashMap<String, Kept> {
    let dirs: HashMap<PathBuf, String> = planned
        .iter()
        .map(|path| (state::feed_dir(root, path), path.clone()))
        .collect();
    let mut kept: HashMap<String, Kept> = HashMap::new();
    let mut stack = vec![root.to_path_buf()];
    let mut seen_dirs = Vec::new();
    while let Some(dir) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for item in read.flatten() {
            let path = item.path();
            if dir == root && (item.file_name() == STATE || item.file_name() == PARTIAL) {
                continue;
            }
            let Ok(meta) = item.metadata() else { continue };
            if meta.is_dir() {
                stack.push(path.clone());
                seen_dirs.push(path);
                continue;
            }
            let recent = meta.modified().is_ok_and(|at| at >= started);
            if keep.contains(&path) || recent {
                if let Some(feed) = dirs.get(&dir) {
                    let is_index =
                        path.file_name()
                            .and_then(|name| name.to_str())
                            .is_some_and(|name| {
                                [PackageManager::Opkg, PackageManager::Apk]
                                    .iter()
                                    .any(|&m| index::files(m).contains(&name))
                            });
                    if !is_index {
                        let entry = kept.entry(feed.clone()).or_default();
                        entry.files += 1;
                        entry.bytes += meta.len();
                    }
                }
                continue;
            }
            if let Err(err) = std::fs::remove_file(&path) {
                tracing::warn!("the feed mirror can't remove {}: {err}", path.display());
            }
        }
    }
    // Deepest first, so a parent is empty once its children are gone.
    seen_dirs.sort_by_key(|dir| std::cmp::Reverse(dir.components().count()));
    for dir in seen_dirs {
        let _ = std::fs::remove_dir(&dir);
    }
    kept
}
