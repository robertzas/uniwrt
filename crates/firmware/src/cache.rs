//! Images to flash (M7.1c1, D-124): downloaded once into a directory of the data dir as
//! `<sha256>.img`, kept only when they match the SHA-256 (and size) their source lists, and
//! reused while kept: the newest [`MAX_CACHED`] stay, by last use. A download is written to a
//! `.part` file first; one left by a stop is deleted when the cache opens.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

use data_encoding::HEXLOWER;
use hyper::header::{HeaderName, HeaderValue};
use uniwrt_model::firmware::FirmwareImage;

use crate::{Error, MAX_UPLOAD_BYTES, http::Http, valid_sha256};

/// The images kept: a rolling upgrade of a few models needs a few.
pub const MAX_CACHED: usize = 4;

/// The images downloaded to flash. Clones share the directory; downloads run one at a time.
#[derive(Clone)]
pub struct ImageCache {
    inner: Arc<Inner>,
}

struct Inner {
    dir: PathBuf,
    http: Http,
    lock: tokio::sync::Mutex<()>,
}

impl ImageCache {
    /// The images in `dir`, created if missing; downloads a stop left unfinished are deleted.
    /// uniwrt calls itself `client` (`uniwrt/<version>`).
    pub async fn open(dir: PathBuf, client: &str) -> Result<Self, Error> {
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|err| storage(&dir, &err))?;
        for (path, _) in entries(&dir, "part").await? {
            let _ = tokio::fs::remove_file(path).await;
        }
        Ok(Self {
            inner: Arc::new(Inner {
                dir,
                http: Http::new(client)?,
                lock: tokio::sync::Mutex::new(()),
            }),
        })
    }

    /// `image`, downloaded from its URL unless kept already.
    pub async fn fetch(&self, image: &FirmwareImage) -> Result<PathBuf, Error> {
        self.fetch_from(image, &image.url, &[]).await
    }

    /// `image`, copied from `from` (a copy kept on this host: the feed mirror's, D-147) unless
    /// kept already; refused unless its SHA-256 and size are the image's.
    pub async fn fetch_local(&self, image: &FirmwareImage, from: &Path) -> Result<PathBuf, Error> {
        let (path, part) = self.paths(image)?;
        let _one = self.inner.lock.lock().await;
        if let Some(kept) = kept(&path, image).await {
            return Ok(kept);
        }
        let copied = copy_hashing(from, &part).await;
        let (size, sha256) = match copied {
            Ok(copied) => copied,
            Err(err) => {
                let _ = tokio::fs::remove_file(&part).await;
                return Err(storage(from, &err));
            }
        };
        self.keep(image, &part, &path, size, &sha256).await
    }

    /// Where `image` is kept, and where it is written first.
    fn paths(&self, image: &FirmwareImage) -> Result<(PathBuf, PathBuf), Error> {
        if !valid_sha256(&image.sha256) {
            return Err(Error::Invalid(format!(
                "{} has no valid SHA-256",
                image.name
            )));
        }
        let dir = &self.inner.dir;
        Ok((
            dir.join(format!("{}.img", image.sha256)),
            dir.join(format!("{}.part", image.sha256)),
        ))
    }

    /// Keeps `part` as `path` once its `size` and `sha256` are `image`'s; else deletes it.
    async fn keep(
        &self,
        image: &FirmwareImage,
        part: &Path,
        path: &Path,
        size: u64,
        sha256: &[u8],
    ) -> Result<PathBuf, Error> {
        let hex = HEXLOWER.encode(sha256);
        if hex != image.sha256 || image.size_bytes.is_some_and(|listed| listed != size) {
            let _ = tokio::fs::remove_file(part).await;
            return Err(Error::Untrusted(format!(
                "{} came as {size} bytes with SHA-256 {hex}, its source lists {} with {}",
                image.name,
                image
                    .size_bytes
                    .map_or_else(|| "no size".to_owned(), |size| format!("{size} bytes")),
                image.sha256
            )));
        }
        tokio::fs::rename(part, path)
            .await
            .map_err(|err| storage(path, &err))?;
        self.evict().await;
        Ok(path.to_path_buf())
    }

    /// `image`, downloaded from `url` (with `headers` on the first request only) unless kept
    /// already; refused unless its SHA-256 and size are the image's.
    pub(crate) async fn fetch_from(
        &self,
        image: &FirmwareImage,
        url: &str,
        headers: &[(HeaderName, HeaderValue)],
    ) -> Result<PathBuf, Error> {
        let (path, part) = self.paths(image)?;
        let _one = self.inner.lock.lock().await;
        if let Some(kept) = kept(&path, image).await {
            return Ok(kept);
        }
        let max = image.size_bytes.unwrap_or(MAX_UPLOAD_BYTES);
        let got = match self
            .inner
            .http
            .download_with(url, headers, max, &part)
            .await
        {
            Ok(got) => got,
            Err(err) => {
                let _ = tokio::fs::remove_file(&part).await;
                return Err(err);
            }
        };
        self.keep(image, &part, &path, got.size, &got.sha256).await
    }

    /// Deletes all but the [`MAX_CACHED`] images used last.
    async fn evict(&self) {
        let Ok(mut kept) = entries(&self.inner.dir, "img").await else {
            return;
        };
        kept.sort_by_key(|(_, used)| std::cmp::Reverse(*used));
        for (path, _) in kept.into_iter().skip(MAX_CACHED) {
            let _ = tokio::fs::remove_file(path).await;
        }
    }
}

/// The files of `dir` ending in `.<extension>`, with when each was last written.
async fn entries(dir: &Path, extension: &str) -> Result<Vec<(PathBuf, SystemTime)>, Error> {
    let mut found = Vec::new();
    let mut read = tokio::fs::read_dir(dir)
        .await
        .map_err(|err| storage(dir, &err))?;
    while let Some(entry) = read.next_entry().await.map_err(|err| storage(dir, &err))? {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == extension)
            && let Ok(meta) = entry.metadata().await
        {
            found.push((path, meta.modified().unwrap_or(SystemTime::UNIX_EPOCH)));
        }
    }
    Ok(found)
}

/// `path`, marked used, when it holds `image` already (by its size; its name is its SHA-256).
async fn kept(path: &Path, image: &FirmwareImage) -> Option<PathBuf> {
    let meta = tokio::fs::metadata(path).await.ok()?;
    if image.size_bytes.is_some_and(|size| size != meta.len()) {
        return None;
    }
    touch(path).await;
    Some(path.to_path_buf())
}

/// Copies `from` to `to`, hashing it; its size and SHA-256.
async fn copy_hashing(from: &Path, to: &Path) -> std::io::Result<(u64, Vec<u8>)> {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let mut source = tokio::fs::File::open(from).await?;
    let mut dest = tokio::fs::File::create(to).await?;
    let mut hash = ring::digest::Context::new(&ring::digest::SHA256);
    let mut buf = vec![0; 256 << 10];
    let mut size = 0;
    loop {
        let n = source.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
        dest.write_all(&buf[..n]).await?;
        size += n as u64;
    }
    dest.flush().await?;
    dest.sync_all().await?;
    Ok((size, hash.finish().as_ref().to_vec()))
}

/// Marks `path` used now, so eviction keeps it.
async fn touch(path: &Path) {
    if let Ok(file) = tokio::fs::File::options().write(true).open(path).await {
        let _ = file.into_std().await.set_modified(SystemTime::now());
    }
}

fn storage(path: &Path, err: &std::io::Error) -> Error {
    Error::Storage(format!("{}: {err}", path.display()))
}

#[cfg(test)]
#[path = "cache_tests.rs"]
mod tests;
