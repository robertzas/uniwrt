//! Restoring a controller backup (D-129): [`unpack`] checks one whole and writes its database
//! and key into a directory; [`Staging`] keeps an uploaded file and the restore it unpacks in
//! `<data dir>/restore` until the next start puts it in place ([`Staging::ready`]).

use std::{
    fs::{self, File},
    io::{self, Read},
    os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _},
    path::{Path, PathBuf},
    sync::Arc,
};

use flate2::read::GzDecoder;
use tokio::task;
use uniwrt_core::SystemClock;
use uniwrt_model::controller_backup::{BackupManifest, RestoreState};
use zeroize::Zeroizing;

use super::{DATABASE, MASTER_KEY, io_err, tar};
use crate::{Error, MasterKey, Result, Store, Vault, age::Decryptor};

/// The manifest's and the key's largest sizes.
const MAX_SMALL: u64 = 64 * 1024;

/// Decrypts a backup from `input` and writes its database and key into `dir` (which must exist),
/// mode 0600; returns its manifest. It is read to its end, so a truncated or altered file fails
/// here; a database newer than this uniwrt's fails with [`Error::TooNew`].
pub fn unpack(input: impl Read, passphrase: &[u8], dir: &Path) -> Result<BackupManifest> {
    let bad = |why: &str| Error::NotBackup(why.to_owned());
    let mut gz = GzDecoder::new(Decryptor::new(input, passphrase)?);
    let read_err =
        |source: io::Error| match source.get_ref().and_then(|e| e.downcast_ref::<Error>()) {
            Some(Error::NotAge(why)) => Error::NotAge(why.clone()),
            _ => Error::NotBackup(format!("cannot read it: {source}")),
        };
    let mut manifest = None;
    let mut key = None;
    let mut database = false;
    while let Some((name, size)) = tar::next_entry(&mut gz).map_err(read_err)? {
        match name.as_str() {
            BackupManifest::NAME | MASTER_KEY if size > MAX_SMALL => {
                return Err(bad("an entry is too large"));
            }
            BackupManifest::NAME if manifest.is_none() => {
                let mut text = Vec::new();
                tar::read_data(&mut gz, size, &mut text).map_err(read_err)?;
                manifest = Some(
                    serde_json::from_slice::<BackupManifest>(&text)
                        .map_err(|_| bad("its manifest is malformed"))?,
                );
            }
            MASTER_KEY if key.is_none() => {
                let mut text = Zeroizing::new(Vec::new());
                tar::read_data(&mut gz, size, &mut *text).map_err(read_err)?;
                let text =
                    std::str::from_utf8(&text).map_err(|_| bad("its master key is malformed"))?;
                key = Some(MasterKey::parse(text).map_err(|_| bad("its master key is malformed"))?);
            }
            DATABASE if !database => {
                let path = dir.join(DATABASE);
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .mode(0o600)
                    .open(&path)
                    .map_err(io_err(&path))?;
                tar::read_data(&mut gz, size, &mut file).map_err(read_err)?;
                file.sync_all().map_err(io_err(&path))?;
                database = true;
            }
            _ => return Err(bad(&format!("unexpected entry {name}"))),
        }
    }
    // The end-of-archive's zero blocks and the gzip stream's end, then the age file's: its last
    // chunk is checked only there.
    let mut padding = ZerosOnly(true);
    io::copy(&mut gz, &mut padding).map_err(read_err)?;
    let rest = io::copy(&mut gz.into_inner(), &mut io::sink()).map_err(read_err)?;
    let (Some(manifest), Some(key), true, true, 0) = (manifest, key, database, padding.0, rest)
    else {
        return Err(bad("it is incomplete"));
    };
    if manifest.format != BackupManifest::FORMAT {
        return Err(bad(&format!(
            "format {} is not one this uniwrt reads",
            manifest.format
        )));
    }
    let supported = crate::migrate::latest();
    if usize::try_from(manifest.schema).map_or(true, |schema| schema > supported) {
        return Err(Error::TooNew {
            found: manifest.schema,
            supported,
        });
    }
    let path = dir.join(MASTER_KEY);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)
        .map_err(io_err(&path))?;
    io::Write::write_all(
        &mut file,
        format!("{}\n", key.to_text().as_str()).as_bytes(),
    )
    .map_err(io_err(&path))?;
    file.sync_all().map_err(io_err(&path))?;
    Ok(manifest)
}

/// A sink that notes whether it was given anything but zeros.
struct ZerosOnly(bool);

impl io::Write for ZerosOnly {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0 &= buf.iter().all(|&b| b == 0);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// A restore unpacked and checked, waiting for the next start.
#[derive(Debug)]
pub struct Staged {
    /// Holds `uniwrt.db` and `master.key`.
    pub dir: PathBuf,
    pub manifest: BackupManifest,
    pub key: MasterKey,
}

/// `<data dir>/restore`: `upload.age` (a file uploaded, its passphrase still to come) and
/// `ready/` (the restore the next start applies).
#[derive(Debug, Clone)]
pub struct Staging {
    root: PathBuf,
}

impl Staging {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            root: data_dir.join("restore"),
        }
    }

    /// Where an upload is written (its directory created).
    pub fn upload_path(&self) -> Result<PathBuf> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.root)
            .map_err(io_err(&self.root))?;
        Ok(self.root.join("upload.age"))
    }

    fn ready_dir(&self) -> PathBuf {
        self.root.join("ready")
    }

    /// What is staged.
    pub async fn state(&self) -> RestoreState {
        let staging = self.clone();
        task::spawn_blocking(move || {
            if let Some(manifest) = read_manifest(&staging.ready_dir()) {
                return RestoreState::Ready { manifest };
            }
            match fs::metadata(staging.root.join("upload.age")) {
                Ok(meta) if meta.is_file() => RestoreState::Uploaded {
                    size_bytes: meta.len(),
                },
                _ => RestoreState::None,
            }
        })
        .await
        .unwrap_or(RestoreState::None)
    }

    /// Unpacks the uploaded file with `passphrase` and checks that its key opens its database;
    /// on success the upload is gone and the restore is ready.
    pub async fn unlock(&self, passphrase: &[u8]) -> Result<BackupManifest> {
        let upload = self.root.join("upload.age");
        let manifest = self.stage(upload.clone(), passphrase).await?;
        let _ = fs::remove_file(upload);
        Ok(manifest)
    }

    /// Unpacks the backup at `file` with `passphrase` and checks that its key opens its database;
    /// replaces a restore already ready.
    pub async fn stage(&self, file: PathBuf, passphrase: &[u8]) -> Result<BackupManifest> {
        let part = self.root.join("ready.part");
        let ready = self.ready_dir();
        let passphrase = Zeroizing::new(passphrase.to_vec());
        let (manifest, key) = task::spawn_blocking({
            let part = part.clone();
            move || -> Result<_> {
                let _ = fs::remove_dir_all(&part);
                fs::DirBuilder::new()
                    .recursive(true)
                    .mode(0o700)
                    .create(&part)
                    .map_err(io_err(&part))?;
                let input = File::open(&file).map_err(io_err(&file))?;
                let manifest = unpack(input, &passphrase, &part)?;
                let text = Zeroizing::new(
                    fs::read_to_string(part.join(MASTER_KEY)).map_err(io_err(&part))?,
                );
                Ok((manifest, MasterKey::parse(&text)?))
            }
        })
        .await
        .map_err(|_| Error::Aborted)?
        .inspect_err(|_| {
            let _ = fs::remove_dir_all(&part);
        })?;
        let checked = async {
            // Migrates the copy to this uniwrt's schema; refuses a key that doesn't open it.
            let store = Store::open(part.join(DATABASE), Arc::new(SystemClock)).await?;
            let opened = Vault::open(&store, &key).await;
            store.close().await;
            opened.map(|_| ())
        }
        .await;
        let finished = checked.and_then(|()| {
            let json = serde_json::to_vec(&manifest).map_err(|source| Error::Encode {
                what: "backup manifest",
                source,
            })?;
            fs::write(part.join(BackupManifest::NAME), json).map_err(io_err(&part))?;
            let _ = fs::remove_dir_all(&ready);
            fs::rename(&part, &ready).map_err(io_err(&ready))
        });
        if finished.is_err() {
            let _ = fs::remove_dir_all(&part);
        }
        finished.map(|()| manifest)
    }

    /// Forgets the upload and the restore.
    pub async fn discard(&self) {
        let root = self.root.clone();
        let _ = task::spawn_blocking(move || {
            let _ = fs::remove_file(root.join("upload.age"));
            let _ = fs::remove_dir_all(root.join("ready.part"));
            let _ = fs::remove_dir_all(root.join("ready"));
        })
        .await;
    }

    /// The restore ready, for the start that applies it (blocking; before the store opens).
    pub fn ready(&self) -> Result<Option<Staged>> {
        let dir = self.ready_dir();
        let Some(manifest) = read_manifest(&dir) else {
            return Ok(None);
        };
        let text = Zeroizing::new(fs::read_to_string(dir.join(MASTER_KEY)).map_err(io_err(&dir))?);
        Ok(Some(Staged {
            key: MasterKey::parse(&text)?,
            dir,
            manifest,
        }))
    }

    /// Where the start that applies a restore keeps the database (and key) it replaced.
    pub fn previous_dir(&self) -> PathBuf {
        self.root.join("previous")
    }
}

fn read_manifest(dir: &Path) -> Option<BackupManifest> {
    serde_json::from_slice(&fs::read(dir.join(BackupManifest::NAME)).ok()?).ok()
}
