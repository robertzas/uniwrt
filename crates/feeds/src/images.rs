//! The official images the mirror keeps for upgrades on offer (D-147, M9.0c1b2): each kept
//! where its URL says, as the feeds are (`<root>/<host>/<path>/<file>`), downloaded once and
//! kept only when its size and SHA-256 are the ones its release lists, and deleted by the sweep
//! once no plan names it. They aren't served to nodes: the upgrade job takes one from the
//! controller's disk ([`Mirror::image`]) and streams it over SSH as any image.

use std::{
    collections::{BTreeMap, HashSet},
    io::Read as _,
    path::{Path, PathBuf},
    sync::Arc,
};

use sha2::{Digest as _, Sha256};
use uniwrt_model::firmware::FirmwareImage;

use crate::{
    Feed, Mirror,
    feed::valid_segment,
    index::unhex32,
    mirror::Inner,
    source::{Fetched, Validators},
    state,
};

/// The largest image kept.
const MAX_IMAGE: u64 = 1 << 30;

/// Where an image at `url` is kept under `root`; `None` for a URL the mirror can't keep.
pub(crate) fn place(root: &Path, url: &str) -> Option<PathBuf> {
    let (dir, file) = url.rsplit_once('/')?;
    if !valid_segment(file) || file == "packages.adb" {
        return None;
    }
    let feed = Feed::parse(dir).ok()?;
    Some(state::feed_dir(root, &feed.mirror_path()).join(file))
}

/// Keeps every image of `images` (its path goes in `keep`); what failed, by URL.
pub(crate) async fn sync(
    inner: &Arc<Inner>,
    images: &[FirmwareImage],
    keep: &mut HashSet<PathBuf>,
    problems: &mut Vec<String>,
) -> BTreeMap<String, String> {
    let mut errors = BTreeMap::new();
    for image in images {
        let result = match place(&inner.root, &image.url) {
            Some(dest) => {
                keep.insert(dest.clone());
                fetch(inner, image, &dest).await
            }
            None => Err("not a URL the mirror can keep".to_owned()),
        };
        if let Err(why) = result {
            problems.push(format!("{}: {why}", image.name));
            errors.insert(image.url.clone(), why);
        }
    }
    errors
}

/// Whether `dest` holds `image` (by its size: it was checked whole when written).
fn holds(dest: &Path, image: &FirmwareImage) -> bool {
    std::fs::metadata(dest)
        .is_ok_and(|meta| meta.is_file() && image.size_bytes.is_none_or(|size| size == meta.len()))
}

/// Downloads `image` to `dest` unless it is there; under the file's lock.
async fn fetch(inner: &Inner, image: &FirmwareImage, dest: &Path) -> Result<(), String> {
    let guard = inner.guard(format!("file:{}", dest.display()));
    let _held = guard.lock().await;
    if holds(dest, image) {
        return Ok(());
    }
    let expected = unhex32(&image.sha256).ok_or("its release lists no valid SHA-256")?;
    let temp = inner.partial();
    let max = image.size_bytes.unwrap_or(MAX_IMAGE).min(MAX_IMAGE);
    let result = async {
        match inner
            .source
            .fetch(&image.url, &Validators::default(), &temp, max)
            .await
        {
            Ok(Fetched::Written { .. }) => {}
            Ok(Fetched::NotModified) => return Err("not modified to a plain request".to_owned()),
            Err(err) => return Err(err.to_string()),
        }
        let path = temp.clone();
        let (size, hash) = tokio::task::spawn_blocking(move || sha256(&path))
            .await
            .map_err(|err| err.to_string())??;
        if hash != expected || image.size_bytes.is_some_and(|listed| listed != size) {
            return Err(format!(
                "downloaded as {size} bytes with another SHA-256 than its release lists"
            ));
        }
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|err| format!("{}: {err}", parent.display()))?;
        }
        tokio::fs::rename(&temp, dest)
            .await
            .map_err(|err| format!("{}: {err}", dest.display()))
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&temp).await;
    }
    result
}

/// `path`'s size and SHA-256. Blocking.
fn sha256(path: &Path) -> Result<(u64, [u8; 32]), String> {
    let mut file = std::fs::File::open(path).map_err(|err| format!("{}: {err}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 << 10];
    let mut size = 0;
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|err| format!("{}: {err}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        size += n as u64;
    }
    Ok((size, hasher.finalize().into()))
}

impl Mirror {
    /// Where the mirror keeps `image`, when a plan names it (its URL and SHA-256) and it is kept
    /// whole: an upgrade takes it from there instead of downloading it.
    pub fn image(&self, image: &FirmwareImage) -> Option<PathBuf> {
        let planned = self
            .inner
            .plan()
            .images
            .iter()
            .any(|kept| kept.url == image.url && kept.sha256 == image.sha256);
        let dest = place(&self.inner.root, &image.url)?;
        (planned && holds(&dest, image)).then_some(dest)
    }
}
