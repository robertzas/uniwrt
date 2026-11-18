//! [`Blocklists`]: the lists' copies in the data directory and their state. A refresh asks the
//! [`Source`] for every wanted list at once, conditionally; a new copy replaces the old one
//! only once it is whole, so a failed download keeps the last good copy, and its error is
//! shown beside it. Copies no longer wanted are deleted. The compile reads the copies kept.
//!
//! On disk: `index.json` (each URL's validators, times, domains and error) and one
//! `<hash>.list` per URL, the body as downloaded.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
};

use serde::{Deserialize, Serialize};
use tokio::task::JoinSet;
use tracing::warn;
use uniwrt_core::Timestamp;
use uniwrt_model::{
    blocklists::ListStatus,
    site::blocking::{Blocking, CATALOG, ListCategory},
};

use crate::{
    Error,
    compile::{Compiled, compile},
    http::{Fetched, Source, Validators},
    parse,
};

const INDEX: &str = "index.json";

/// A list to keep: the catalog's (by id) or the user's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wanted {
    pub id: Option<String>,
    pub name: String,
    pub url: String,
}

/// The lists `blocking` names, most wanted first: the catalog's malware lists, the user's own,
/// then the catalog's ads lists (the order the gateway's capacity cuts from the end); each URL
/// once. Unknown catalog ids are skipped.
pub fn wanted(blocking: &Blocking) -> Vec<Wanted> {
    let catalog = |category: ListCategory| {
        CATALOG
            .iter()
            .filter(move |list| list.category == category)
            .filter(|list| blocking.lists.iter().any(|id| id == list.id))
            .map(|list| Wanted {
                id: Some(list.id.to_owned()),
                name: list.name.to_owned(),
                url: list.url.to_owned(),
            })
    };
    let custom = blocking.custom.iter().map(|list| Wanted {
        id: None,
        name: list.name.clone(),
        url: list.url.clone(),
    });
    let mut seen = BTreeSet::new();
    catalog(ListCategory::Malware)
        .chain(custom)
        .chain(catalog(ListCategory::Ads))
        .filter(|list| seen.insert(list.url.clone()))
        .collect()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Entry {
    #[serde(default)]
    etag: Option<String>,
    #[serde(default)]
    last_modified: Option<String>,
    /// When it was last asked for, whatever came of it.
    #[serde(default)]
    attempted_at: Option<Timestamp>,
    #[serde(default)]
    checked_at: Option<Timestamp>,
    #[serde(default)]
    fetched_at: Option<Timestamp>,
    #[serde(default)]
    domains: Option<u32>,
    #[serde(default)]
    error: Option<String>,
}

/// The lists' copies and their state. Clones share them.
#[derive(Clone)]
pub struct Blocklists(Arc<Inner>);

struct Inner {
    dir: PathBuf,
    source: Arc<dyn Source>,
    index: Mutex<BTreeMap<String, Entry>>,
    /// One refresh at a time.
    refreshing: tokio::sync::Mutex<()>,
}

impl Blocklists {
    /// Opens (or creates) the copies in `dir`; an unreadable index starts empty.
    pub async fn open(dir: PathBuf, source: Arc<dyn Source>) -> Result<Self, Error> {
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|err| storage(&dir, &err))?;
        let index = match tokio::fs::read(dir.join(INDEX)).await {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|err| {
                warn!(%err, "blocklists: the index is unreadable; starting again");
                BTreeMap::new()
            }),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(err) => return Err(storage(&dir.join(INDEX), &err)),
        };
        Ok(Self(Arc::new(Inner {
            dir,
            source,
            index: Mutex::new(index),
            refreshing: tokio::sync::Mutex::new(()),
        })))
    }

    /// The state of each of `wanted`.
    pub fn status(&self, wanted: &[Wanted]) -> Vec<ListStatus> {
        let index = self.index();
        wanted
            .iter()
            .map(|list| {
                let entry = index.get(&list.url).cloned().unwrap_or_default();
                ListStatus {
                    id: list.id.clone(),
                    name: list.name.clone(),
                    url: list.url.clone(),
                    checked_at: entry.checked_at,
                    fetched_at: entry.fetched_at,
                    domains: entry.domains,
                    error: entry.error,
                }
            })
            .collect()
    }

    /// When the least recently asked for of `wanted` was asked for; `None` while one never was.
    pub fn attempted(&self, wanted: &[Wanted]) -> Option<Timestamp> {
        let index = self.index();
        wanted
            .iter()
            .map(|list| index.get(&list.url).and_then(|entry| entry.attempted_at))
            .try_fold(None, |oldest: Option<Timestamp>, at| {
                at.map(|at| Some(oldest.map_or(at, |oldest| oldest.min(at))))
            })
            .flatten()
    }

    /// Asks for every list of `wanted` again and deletes the copies of the others. True when a
    /// copy changed.
    pub async fn refresh(&self, wanted: &[Wanted], now: Timestamp) -> bool {
        let _one = self.0.refreshing.lock().await;
        let mut downloads = JoinSet::new();
        for list in wanted {
            let validators = self
                .index()
                .get(&list.url)
                .map_or_else(Validators::default, |e| Validators {
                    etag: e.etag.clone(),
                    last_modified: e.last_modified.clone(),
                });
            let inner = Arc::clone(&self.0);
            let url = list.url.clone();
            downloads.spawn(async move {
                let fetched = inner.source.fetch(&url, &validators).await;
                let kept = match fetched {
                    Ok(Fetched::Body { body, validators }) => keep(&inner.dir, &url, body)
                        .await
                        .map(|n| Some((n, validators))),
                    Ok(Fetched::NotModified) => Ok(None),
                    Err(err) => Err(err),
                };
                (url, kept)
            });
        }
        let mut changed = false;
        while let Some(done) = downloads.join_next().await {
            let Ok((url, kept)) = done else { continue };
            let mut index = self.index();
            let entry = index.entry(url.clone()).or_default();
            entry.attempted_at = Some(now);
            match kept {
                Ok(new) => {
                    entry.checked_at = Some(now);
                    entry.error = None;
                    if let Some((domains, validators)) = new {
                        changed = true;
                        entry.fetched_at = Some(now);
                        entry.domains = Some(domains);
                        entry.etag = validators.etag;
                        entry.last_modified = validators.last_modified;
                    }
                }
                Err(err) => {
                    warn!(%url, %err, "blocklists: cannot refresh a list");
                    entry.error = Some(err.to_string());
                }
            }
        }
        let gone: Vec<String> = {
            let urls: BTreeSet<&str> = wanted.iter().map(|list| list.url.as_str()).collect();
            let mut index = self.index();
            let gone: Vec<String> = index
                .keys()
                .filter(|url| !urls.contains(url.as_str()))
                .cloned()
                .collect();
            for url in &gone {
                index.remove(url);
            }
            gone
        };
        for url in &gone {
            let _ = tokio::fs::remove_file(self.0.dir.join(file_name(url))).await;
        }
        if let Err(err) = self.save().await {
            warn!(%err, "blocklists: cannot save the index");
        }
        changed
    }

    /// Compiles the copies of `wanted` with `blocking`'s own domains into at most `capacity`
    /// domains; a list without a copy is left out.
    pub async fn compile(
        &self,
        wanted: &[Wanted],
        blocking: &Blocking,
        capacity: u32,
    ) -> Result<Compiled, Error> {
        let mut bodies = Vec::with_capacity(wanted.len());
        for list in wanted {
            let path = self.0.dir.join(file_name(&list.url));
            match tokio::fs::read(&path).await {
                Ok(body) => bodies.push(body),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => return Err(storage(&path, &err)),
            }
        }
        let (allow, deny) = (blocking.allow.clone(), blocking.deny.clone());
        tokio::task::spawn_blocking(move || {
            let lists: Vec<&[u8]> = bodies.iter().map(Vec::as_slice).collect();
            compile(&lists, &allow, &deny, capacity)
        })
        .await
        .map_err(|err| Error::Invalid(format!("the compile stopped: {err}")))
    }

    fn index(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Entry>> {
        self.0.index.lock().unwrap_or_else(PoisonError::into_inner)
    }

    async fn save(&self) -> Result<(), Error> {
        let bytes = serde_json::to_vec_pretty(&*self.index())
            .map_err(|err| Error::Storage(err.to_string()))?;
        write(&self.0.dir.join(INDEX), &bytes).await
    }
}

/// Writes a new copy of `url`, once whole; its domain count.
async fn keep(dir: &Path, url: &str, body: bytes::Bytes) -> Result<u32, Error> {
    let counted = body.clone();
    let domains = tokio::task::spawn_blocking(move || count(&counted))
        .await
        .map_err(|err| Error::Invalid(format!("the count stopped: {err}")))?;
    write(&dir.join(file_name(url)), &body).await?;
    Ok(domains)
}

fn count(body: &[u8]) -> u32 {
    let mut names = Vec::new();
    let mut n = 0_u32;
    for line in body.split(|&b| b == b'\n') {
        parse::line(line, &mut names);
        n = n.saturating_add(u32::try_from(names.len()).unwrap_or(u32::MAX));
        names.clear();
    }
    n
}

async fn write(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let tmp = path.with_extension("tmp");
    tokio::fs::write(&tmp, bytes)
        .await
        .map_err(|err| storage(&tmp, &err))?;
    tokio::fs::rename(&tmp, path)
        .await
        .map_err(|err| storage(path, &err))
}

/// The copy's file: a hash of its URL, which may hold any character.
fn file_name(url: &str) -> String {
    use std::fmt::Write as _;

    use sha2::{Digest as _, Sha256};
    let digest = Sha256::digest(url.as_bytes());
    let mut name = digest[..8].iter().fold(String::new(), |mut hex, b| {
        let _ = write!(hex, "{b:02x}");
        hex
    });
    name.push_str(".list");
    name
}

fn storage(path: &Path, err: &std::io::Error) -> Error {
    Error::Storage(format!("{}: {err}", path.display()))
}

#[cfg(test)]
#[path = "lists_tests.rs"]
mod tests;
