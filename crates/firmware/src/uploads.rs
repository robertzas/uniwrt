//! Uploaded images (M7.1a2, D-122): files in a directory of the data dir, each `<sha256>.img`
//! beside `<sha256>.json` (its [`FirmwareUpload`]). An upload is written to a `.part` file
//! while it is hashed, inspected once complete ([`crate::image`]), then renamed: a file that
//! isn't an image the controller can match to boards is never kept.

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use tokio::io::AsyncWriteExt as _;
use uniwrt_core::Timestamp;
use uniwrt_model::firmware::FirmwareUpload;

use crate::{
    Error,
    image::{self, TAIL},
};

/// The largest image taken (x86's ext4 images are the largest, tens of MiB).
pub const MAX_UPLOAD_BYTES: u64 = 256 << 20;
/// The images kept at most: older ones are deleted by hand.
pub const MAX_UPLOADS: usize = 16;
/// The first bytes kept to tell a gzipped image.
const HEAD: usize = 16;

/// The uploaded images. Clones share the directory.
#[derive(Clone)]
pub struct Uploads {
    inner: Arc<Inner>,
}

struct Inner {
    dir: PathBuf,
    next: AtomicU64,
    /// Held while an upload is kept or one deleted, so the count holds.
    lock: tokio::sync::Mutex<()>,
}

impl Uploads {
    /// The images in `dir`, created if missing; unfinished uploads left by a stop are deleted.
    pub async fn open(dir: PathBuf) -> Result<Self, Error> {
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|err| storage(&dir, &err))?;
        let mut entries = tokio::fs::read_dir(&dir)
            .await
            .map_err(|err| storage(&dir, &err))?;
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|err| storage(&dir, &err))?
        {
            if entry.path().extension().is_some_and(|ext| ext == "part") {
                let _ = tokio::fs::remove_file(entry.path()).await;
            }
        }
        Ok(Self {
            inner: Arc::new(Inner {
                dir,
                next: AtomicU64::new(0),
                lock: tokio::sync::Mutex::new(()),
            }),
        })
    }

    /// Every image, newest first. A description that can't be read is skipped.
    pub async fn list(&self) -> Result<Vec<FirmwareUpload>, Error> {
        let dir = &self.inner.dir;
        let mut entries = tokio::fs::read_dir(dir)
            .await
            .map_err(|err| storage(dir, &err))?;
        let mut found = Vec::new();
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|err| storage(dir, &err))?
        {
            let path = entry.path();
            let Some(sha256) = path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_suffix(".json"))
            else {
                continue;
            };
            if valid_sha256(sha256)
                && let Ok(Some(upload)) = self.get(sha256).await
            {
                found.push(upload);
            }
        }
        found.sort_by(|a, b| b.uploaded_at.cmp(&a.uploaded_at).then(a.name.cmp(&b.name)));
        Ok(found)
    }

    /// The image `sha256`, if it is kept.
    pub async fn get(&self, sha256: &str) -> Result<Option<FirmwareUpload>, Error> {
        if !valid_sha256(sha256) {
            return Ok(None);
        }
        let path = self.inner.dir.join(format!("{sha256}.json"));
        let body = match tokio::fs::read(&path).await {
            Ok(body) => body,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(storage(&path, &err)),
        };
        if tokio::fs::metadata(self.image(sha256)).await.is_err() {
            return Ok(None);
        }
        serde_json::from_slice(&body)
            .map(Some)
            .map_err(|err| Error::Storage(format!("{} is unreadable: {err}", path.display())))
    }

    /// Where the image `sha256` is (installing it streams it from there, M7.1c).
    pub fn path(&self, sha256: &str) -> Option<PathBuf> {
        valid_sha256(sha256).then(|| self.image(sha256))
    }

    /// Deletes the image `sha256`; false when there was none.
    pub async fn remove(&self, sha256: &str) -> Result<bool, Error> {
        let _held = self.inner.lock.lock().await;
        if self.get(sha256).await?.is_none() {
            return Ok(false);
        }
        let description = self.inner.dir.join(format!("{sha256}.json"));
        tokio::fs::remove_file(&description)
            .await
            .map_err(|err| storage(&description, &err))?;
        let _ = tokio::fs::remove_file(self.image(sha256)).await;
        Ok(true)
    }

    /// Starts receiving an image named `name` (the file's name, for people and for an x86
    /// image's target).
    pub async fn receive(&self, name: &str) -> Result<Receiving, Error> {
        let name = image::file_name(name)?;
        if self.list().await?.len() >= MAX_UPLOADS {
            return Err(Error::Invalid(format!(
                "{MAX_UPLOADS} images are kept at most: delete one first"
            )));
        }
        let serial = self.inner.next.fetch_add(1, Ordering::Relaxed);
        let path = self
            .inner
            .dir
            .join(format!("upload-{}-{serial}.part", std::process::id()));
        let file = tokio::fs::File::create(&path)
            .await
            .map_err(|err| storage(&path, &err))?;
        Ok(Receiving {
            uploads: self.clone(),
            name,
            file: Some(file),
            path,
            hash: ring::digest::Context::new(&ring::digest::SHA256),
            size: 0,
            head: Vec::with_capacity(HEAD),
            tail: VecDeque::with_capacity(TAIL),
        })
    }

    fn image(&self, sha256: &str) -> PathBuf {
        self.inner.dir.join(format!("{sha256}.img"))
    }
}

/// An image being received: [`Receiving::write`] each part, then [`Receiving::finish`].
/// Dropped unfinished, its part file is deleted.
pub struct Receiving {
    uploads: Uploads,
    name: String,
    file: Option<tokio::fs::File>,
    path: PathBuf,
    hash: ring::digest::Context,
    size: u64,
    head: Vec<u8>,
    tail: VecDeque<u8>,
}

impl Receiving {
    pub async fn write(&mut self, part: &[u8]) -> Result<(), Error> {
        self.size += part.len() as u64;
        if self.size > MAX_UPLOAD_BYTES {
            return Err(Error::Invalid(format!(
                "The image is larger than {} MiB",
                MAX_UPLOAD_BYTES >> 20
            )));
        }
        self.hash.update(part);
        let room = HEAD.saturating_sub(self.head.len());
        self.head.extend_from_slice(&part[..room.min(part.len())]);
        let keep = &part[part.len().saturating_sub(TAIL)..];
        let excess = (self.tail.len() + keep.len()).saturating_sub(TAIL);
        self.tail.drain(..excess);
        self.tail.extend(keep);
        let file = self
            .file
            .as_mut()
            .ok_or_else(|| Error::Storage("the upload is closed".to_owned()))?;
        file.write_all(part)
            .await
            .map_err(|err| storage(&self.path, &err))
    }

    /// Inspects the image and keeps it; an image already kept is kept once.
    pub async fn finish(mut self, at: Timestamp) -> Result<FirmwareUpload, Error> {
        let mut file = self
            .file
            .take()
            .ok_or_else(|| Error::Storage("the upload is closed".to_owned()))?;
        file.flush()
            .await
            .map_err(|err| storage(&self.path, &err))?;
        file.sync_all()
            .await
            .map_err(|err| storage(&self.path, &err))?;
        drop(file);
        if self.size == 0 {
            return Err(Error::Invalid("The image is empty".to_owned()));
        }
        let tail: Vec<u8> = self.tail.iter().copied().collect();
        let found = image::inspect(&self.name, &self.head, &tail)?;
        let sha256 = hex(self.hash.clone().finish().as_ref());
        let uploads = self.uploads.clone();
        let _held = uploads.inner.lock.lock().await;
        if let Some(kept) = uploads.get(&sha256).await? {
            return Ok(kept);
        }
        let upload = FirmwareUpload {
            sha256: sha256.clone(),
            name: self.name.clone(),
            size_bytes: self.size,
            uploaded_at: at,
            target: found.target,
            boards: found.boards,
            distribution: found.distribution,
            version: found.version,
            revision: found.revision,
        };
        let image = uploads.image(&sha256);
        tokio::fs::rename(&self.path, &image)
            .await
            .map_err(|err| storage(&image, &err))?;
        let description = uploads.inner.dir.join(format!("{sha256}.json"));
        let written = write_atomic(
            &description,
            &serde_json::to_vec(&upload).map_err(|err| Error::Storage(err.to_string()))?,
        )
        .await;
        if let Err(err) = written {
            let _ = tokio::fs::remove_file(&image).await;
            return Err(err);
        }
        Ok(upload)
    }
}

impl Drop for Receiving {
    fn drop(&mut self) {
        // After `finish` renamed it, the part is gone and this fails quietly.
        let _ = std::fs::remove_file(&self.path);
    }
}

async fn write_atomic(path: &Path, body: &[u8]) -> Result<(), Error> {
    let part = path.with_extension("json.part");
    tokio::fs::write(&part, body)
        .await
        .map_err(|err| storage(&part, &err))?;
    tokio::fs::rename(&part, path)
        .await
        .map_err(|err| storage(path, &err))
}

/// Whether `upload` is made for a node of `target` on `board`: its metadata lists the board
/// (and names the same target), or it is an x86 image of the node's subtarget.
pub fn upload_fits(upload: &FirmwareUpload, target: &str, board: &str) -> bool {
    upload.target == target
        && (upload.boards.iter().any(|known| known == board)
            || (upload.boards.is_empty() && target.starts_with("x86/")))
}

/// A SHA-256 as the uploads are named: 64 lowercase hex digits.
pub fn valid_sha256(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::with_capacity(64), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

fn storage(path: &Path, err: &std::io::Error) -> Error {
    Error::Storage(format!("{}: {err}", path.display()))
}

#[cfg(test)]
#[path = "uploads_tests.rs"]
mod tests;
