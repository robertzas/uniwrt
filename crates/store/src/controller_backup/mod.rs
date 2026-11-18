//! Controller backups (features/lifecycle.md, Backups; D-126, D-129): [`ControllerBackups`]
//! takes one as an age-encrypted gzip tarball of [`BackupManifest::NAME`], `master.key` and
//! `uniwrt.db` (an online `VACUUM INTO` snapshot, read on the read pool so writes go on), named
//! `uniwrt-backup-<UTC time>.tar.gz.age`, into the backup directory and then the extra one,
//! keeping the newest [`KEEP`] in each. The entries' names are a data directory's, so
//! `age -d FILE | tar xz` in one restores it by hand. [`Staging`] unpacks one for a restore.
//!
//! Files appear whole: each is written as `.part` and renamed. The plaintext snapshot lives only
//! in the (local) backup directory, mode 0600, until it is encrypted.

mod stage;
mod tar;
#[cfg(test)]
mod tests;

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write as _},
    os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _},
    path::{Path, PathBuf},
    sync::Arc,
};

use flate2::{Compression, write::GzEncoder};
use tokio::{sync::Mutex, task};
use tracing::warn;
use uniwrt_core::Timestamp;
use uniwrt_model::controller_backup::{
    BackupManifest, BackupOutcome, ControllerBackup, KEEP, PASSPHRASE_SECRET,
};

pub use stage::{Staged, Staging, unpack};

use crate::{
    Error, MasterKey, Result, SettingsRepo as _, Store, Vault, VaultRepo as _,
    age::{Encryptor, LOG_N},
};

const PREFIX: &str = "uniwrt-backup-";
const SUFFIX: &str = ".tar.gz.age";
/// The tarball's entries besides the manifest.
pub const DATABASE: &str = "uniwrt.db";
pub const MASTER_KEY: &str = "master.key";

/// The backup directories; one backup at a time. Clones share them.
#[derive(Debug, Clone)]
pub struct ControllerBackups {
    dir: PathBuf,
    extra: Option<PathBuf>,
    busy: Arc<Mutex<()>>,
    log_n: u8,
}

/// A backup taken, and why its copy to the extra directory failed, if it did.
#[derive(Debug)]
pub struct Taken {
    pub backup: ControllerBackup,
    pub copy_error: Option<String>,
}

impl ControllerBackups {
    /// Backups in `dir` (local: the snapshot passes through it) and copied to `extra`.
    pub fn new(dir: PathBuf, extra: Option<PathBuf>) -> Self {
        Self {
            dir,
            extra,
            busy: Arc::default(),
            log_n: LOG_N,
        }
    }

    /// The same with scrypt's work factor `log_n` instead of age's 18: cheap ones for tests.
    #[must_use]
    pub fn with_work_factor(mut self, log_n: u8) -> Self {
        self.log_n = log_n;
        self
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn extra(&self) -> Option<&Path> {
        self.extra.as_deref()
    }

    /// Whether a backup is being taken.
    pub fn running(&self) -> bool {
        self.busy.try_lock().is_err()
    }

    /// Takes a backup of `store` and `key` under `passphrase`; `version` is uniwrt's. Fails with
    /// [`Error::Conflict`] while another is taken.
    pub async fn take(
        &self,
        store: &Store,
        key: &MasterKey,
        passphrase: &[u8],
        version: &str,
    ) -> Result<Taken> {
        let _busy = self.busy.try_lock().map_err(|_| Error::Conflict {
            what: "controller backup",
            key: "now".to_owned(),
        })?;
        let now = store.now();
        let name = file_name(now);
        let dir = self.dir.clone();
        let snapshot = dir.join(format!(".snapshot-{}.db", now.as_unix_ms()));
        task::spawn_blocking({
            let (dir, snapshot) = (dir.clone(), snapshot.clone());
            move || -> Result<()> {
                fs::DirBuilder::new()
                    .recursive(true)
                    .mode(0o700)
                    .create(&dir)
                    .map_err(io_err(&dir))?;
                remove_leftovers(&dir);
                // VACUUM INTO takes an empty file as it is, so the snapshot is private from the start.
                private_file(&snapshot).map_err(io_err(&snapshot))?;
                Ok(())
            }
        })
        .await
        .map_err(|_| Error::Aborted)??;
        let result = async {
            store.snapshot_into(snapshot.clone()).await?;
            let manifest = BackupManifest {
                format: BackupManifest::FORMAT,
                version: version.to_owned(),
                schema: i64::try_from(crate::migrate::latest()).unwrap_or(i64::MAX),
                taken_at: now,
            };
            let key_text = key.to_text();
            let passphrase = zeroize::Zeroizing::new(passphrase.to_vec());
            let (target, snapshot, log_n) = (dir.join(&name), snapshot.clone(), self.log_n);
            task::spawn_blocking(move || {
                write_backup(&target, &snapshot, &manifest, &key_text, &passphrase, log_n)
            })
            .await
            .map_err(|_| Error::Aborted)?
        }
        .await;
        let _ = fs::remove_file(&snapshot);
        let size_bytes = result?;
        let copy_error = match &self.extra {
            Some(extra) => {
                let shown = extra.display().to_string();
                let (from, extra, name) = (dir.join(&name), extra.clone(), name.clone());
                task::spawn_blocking(move || copy_to(&from, &extra, &name))
                    .await
                    .map_err(|_| Error::Aborted)?
                    .err()
                    .map(|err| format!("cannot copy it to {shown}: {}", chain(&err)))
            }
            None => None,
        };
        let dirs: Vec<PathBuf> = std::iter::once(dir).chain(self.extra.clone()).collect();
        task::spawn_blocking(move || dirs.iter().for_each(|dir| prune(dir)))
            .await
            .map_err(|_| Error::Aborted)?;
        Ok(Taken {
            backup: ControllerBackup {
                copied: self.extra.is_some() && copy_error.is_none(),
                name,
                taken_at: Timestamp::from_unix_secs(now.as_unix_ms().div_euclid(1_000)),
                size_bytes,
            },
            copy_error,
        })
    }

    /// Takes a backup under the passphrase the vault holds and records how it went
    /// ([`BackupOutcome`], unless another was being taken); [`Error::NotFound`] when no
    /// passphrase is set.
    pub async fn run(&self, store: &Store, vault: &Vault, version: &str) -> Result<Taken> {
        let Some(passphrase) = vault.secret(PASSPHRASE_SECRET).await? else {
            return Err(Error::NotFound {
                what: "backup passphrase",
                key: PASSPHRASE_SECRET.to_owned(),
            });
        };
        let result = self
            .take(store, vault.master_key(), &passphrase, version)
            .await;
        if matches!(result, Err(Error::Conflict { .. })) {
            return result;
        }
        let outcome = BackupOutcome {
            at: store.now(),
            error: match &result {
                Ok(taken) => taken.copy_error.clone(),
                Err(err) => Some(chain(err)),
            },
        };
        let recorded = match serde_json::to_value(&outcome) {
            Ok(value) => store.put_setting(BackupOutcome::KEY, value).await,
            Err(source) => Err(Error::Encode {
                what: "backup outcome",
                source,
            }),
        };
        if let Err(err) = recorded {
            warn!(%err, "cannot record how the controller backup went");
        }
        result
    }

    /// The backups in the backup directory, newest first.
    pub async fn list(&self) -> Result<Vec<ControllerBackup>> {
        let (dir, extra) = (self.dir.clone(), self.extra.clone());
        task::spawn_blocking(move || {
            let mut backups: Vec<ControllerBackup> = backups_in(&dir)
                .into_iter()
                .filter_map(|(name, taken_at)| {
                    let size_bytes = fs::metadata(dir.join(&name)).ok()?.len();
                    let copied = extra
                        .as_ref()
                        .is_some_and(|extra| extra.join(&name).is_file());
                    Some(ControllerBackup {
                        name,
                        taken_at,
                        size_bytes,
                        copied,
                    })
                })
                .collect();
            backups.sort_by_key(|b| std::cmp::Reverse(b.taken_at));
            backups
        })
        .await
        .map_err(|_| Error::Aborted)
    }

    /// The file of backup `name`, if it is one of the backup directory's (or, lost there, the
    /// extra one's).
    pub fn file(&self, name: &str) -> Option<PathBuf> {
        parse_name(name)?;
        std::iter::once(&self.dir)
            .chain(&self.extra)
            .map(|dir| dir.join(name))
            .find(|path| path.is_file())
    }
}

impl Store {
    /// Writes a consistent copy of the database to `path` (an empty file) with `VACUUM INTO`, on
    /// a read connection so that writes go on meanwhile.
    pub(crate) async fn snapshot_into(&self, path: PathBuf) -> Result<()> {
        self.read(move |conn| {
            let path = path.to_str().ok_or_else(|| {
                Error::NotBackup("the backup directory's path is not UTF-8".to_owned())
            })?;
            conn.execute("VACUUM INTO ?1", [path])?;
            Ok(())
        })
        .await
    }
}

/// The file name of a backup taken at `at`, to the second.
fn file_name(at: Timestamp) -> String {
    let time = Timestamp::from_unix_secs(at.as_unix_ms().div_euclid(1_000)).to_string();
    format!("{PREFIX}{}{SUFFIX}", time.replace(':', ""))
}

/// When the backup named `name` was taken, if it is a backup's name.
fn parse_name(name: &str) -> Option<Timestamp> {
    let time = name.strip_prefix(PREFIX)?.strip_suffix(SUFFIX)?;
    // 2026-11-18T020000Z
    if time.len() != 18 || !time.is_ascii() {
        return None;
    }
    Timestamp::parse_utc(&format!(
        "{}:{}:{}",
        &time[..13],
        &time[13..15],
        &time[15..]
    ))
}

fn backups_in(dir: &Path) -> Vec<(String, Timestamp)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().into_string().ok()?;
            let at = parse_name(&name)?;
            Some((name, at))
        })
        .collect()
}

/// Deletes all but the newest [`KEEP`] backups in `dir`.
fn prune(dir: &Path) {
    let mut backups = backups_in(dir);
    backups.sort_by_key(|b| std::cmp::Reverse(b.1));
    for (name, _) in backups.into_iter().skip(KEEP) {
        if let Err(err) = fs::remove_file(dir.join(&name)) {
            warn!(%err, name, "cannot delete an old controller backup");
        }
    }
}

/// Deletes what an interrupted backup left: snapshots and partial files.
fn remove_leftovers(dir: &Path) {
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if (name.starts_with(".snapshot-") && name.ends_with(".db"))
            || (name.starts_with(PREFIX) && name.ends_with(".part"))
        {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn private_file(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

/// Writes the backup to `target` through a `.part` file; returns its size.
fn write_backup(
    target: &Path,
    snapshot: &Path,
    manifest: &BackupManifest,
    key_text: &str,
    passphrase: &[u8],
    log_n: u8,
) -> Result<u64> {
    let part = part_path(target);
    let written = (|| -> Result<u64> {
        let file = private_file(&part).map_err(io_err(&part))?;
        let age = Encryptor::new(file, passphrase, log_n)?;
        let mut gz = GzEncoder::new(age, Compression::default());
        let mtime = manifest.taken_at.as_unix_ms().div_euclid(1_000);
        let manifest = serde_json::to_vec_pretty(manifest).map_err(|source| Error::Encode {
            what: "backup manifest",
            source,
        })?;
        let key = format!("{key_text}\n");
        let mut database = File::open(snapshot).map_err(io_err(snapshot))?;
        let len = database.metadata().map_err(io_err(snapshot))?.len();
        (|| -> io::Result<File> {
            tar::write_file(
                &mut gz,
                BackupManifest::NAME,
                manifest.len() as u64,
                mtime,
                &mut manifest.as_slice(),
            )?;
            tar::write_file(
                &mut gz,
                MASTER_KEY,
                key.len() as u64,
                mtime,
                &mut key.as_bytes(),
            )?;
            tar::write_file(&mut gz, DATABASE, len, mtime, &mut database)?;
            tar::finish(&mut gz)?;
            let file = gz.finish()?.finish()?;
            file.sync_all()?;
            Ok(file)
        })()
        .map_err(io_err(&part))?;
        drop(zeroize::Zeroizing::new(key));
        let size = fs::metadata(&part).map_err(io_err(&part))?.len();
        fs::rename(&part, target).map_err(io_err(target))?;
        Ok(size)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&part);
    }
    written
}

/// Copies `from` into `dir` as `name`, through a `.part` file.
fn copy_to(from: &Path, dir: &Path, name: &str) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let target = dir.join(name);
    let part = part_path(&target);
    let copied = (|| {
        let mut out = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&part)?;
        io::copy(&mut File::open(from)?, &mut out)?;
        out.flush()?;
        out.sync_all()?;
        fs::rename(&part, &target)
    })();
    if copied.is_err() {
        let _ = fs::remove_file(&part);
    }
    copied
}

fn part_path(target: &Path) -> PathBuf {
    let mut part = target.as_os_str().to_owned();
    part.push(".part");
    PathBuf::from(part)
}

pub(super) fn io_err(path: &Path) -> impl Fn(io::Error) -> Error + '_ {
    move |source| Error::Io {
        what: path.display().to_string(),
        source,
    }
}

/// An error and its causes, for people.
fn chain(err: &dyn std::error::Error) -> String {
    let mut text = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}
