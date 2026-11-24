//! The package feed mirror's copies (D-147): in `<data>/feeds`, downloaded over HTTP(S); demo
//! mode's come from [`DemoFeeds`] instead, with no internet.

use std::{io::Write as _, path::Path, sync::Arc};

use anyhow::Context as _;
use flate2::{Compression, write::GzEncoder};
use sha2::{Digest as _, Sha256};
use uniwrt_core::SystemClock;
use uniwrt_feeds::{BoxFuture, Error, Fetched, HttpSource, Mirror, Source, Validators};
use uniwrt_model::diag::tools::OptionalTool;

use crate::build_info;

/// The copies' directory in the data dir.
const DIR: &str = "feeds";

/// Opens the copies in `state_dir`, downloaded by `demo`'s source or over HTTP(S).
pub(crate) async fn open(state_dir: &Path, demo: bool) -> anyhow::Result<Mirror> {
    let source: Arc<dyn Source> = if demo {
        Arc::new(DemoFeeds)
    } else {
        HttpSource::new(&format!("uniwrt/{}", build_info::VERSION))
            .context("cannot set up the feeds' downloads")?
    };
    Mirror::open(state_dir.join(DIR), source, Arc::new(SystemClock))
        .await
        .context("cannot open the feed mirror")
}

/// Demo mode's feeds: an opkg feed named `base` lists every package uniwrt installs (each a few
/// KiB of made-up bytes, depending on `libc`), every other opkg feed is empty, and apk's
/// binary indexes aren't made up: they are unreachable. Asked again, an index is unchanged.
pub(crate) struct DemoFeeds;

const ETAG: &str = "\"demo\"";

impl DemoFeeds {
    fn names() -> Vec<&'static str> {
        let mut names = uniwrt_render::every_package();
        names.extend(OptionalTool::ALL.map(OptionalTool::package));
        names.extend(["kmod-batman-adv", "batctl-default", "libc"]);
        names
    }

    fn file(name: &str) -> String {
        format!("{name}_1.0-r1_all.ipk")
    }

    /// A package's made-up bytes: 2 to 18 KiB, the same for the same file.
    fn package(file: &str) -> Vec<u8> {
        let size = 2048 + (file.len() * 997) % 16_384;
        file.bytes().cycle().take(size).collect()
    }

    fn index(url: &str) -> Vec<u8> {
        use std::fmt::Write as _;

        let mut text = String::new();
        if url.ends_with("/base/Packages.gz") {
            for name in Self::names() {
                let file = Self::file(name);
                let body = Self::package(&file);
                let depends = if name == "libc" { "" } else { "libc" };
                let hash = Sha256::digest(&body)
                    .iter()
                    .fold(String::new(), |mut hex, byte| {
                        let _ = write!(hex, "{byte:02x}");
                        hex
                    });
                let _ = write!(
                    text,
                    "Package: {name}\nVersion: 1.0-r1\nDepends: {depends}\nFilename: {file}\n\
                     Size: {}\nSHA256sum: {hash}\nDescription: {name}, simulated\n\n",
                    body.len()
                );
            }
        }
        let mut gz = GzEncoder::new(Vec::new(), Compression::fast());
        let _ = gz.write_all(text.as_bytes());
        gz.finish().unwrap_or_default()
    }
}

impl Source for DemoFeeds {
    fn fetch<'a>(
        &'a self,
        url: &'a str,
        validators: &'a Validators,
        dest: &'a Path,
        _max: u64,
    ) -> BoxFuture<'a, Result<Fetched, Error>> {
        Box::pin(async move {
            let index = url.ends_with("/Packages.gz") || url.ends_with("/Packages.sig");
            if index && validators.etag.as_deref() == Some(ETAG) {
                return Ok(Fetched::NotModified);
            }
            let body = if url.ends_with("/Packages.gz") {
                Self::index(url)
            } else if url.ends_with("/Packages.sig") {
                b"demo signature\n".to_vec()
            } else if let Some(file) = url.rsplit('/').next().filter(|f| {
                Path::new(f)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("ipk"))
            }) {
                Self::package(file)
            } else {
                return Err(Error::Unreachable {
                    url: url.to_owned(),
                    why: "demo mode makes up opkg feeds only".to_owned(),
                });
            };
            tokio::fs::write(dest, &body)
                .await
                .map_err(|err| Error::Storage(format!("{}: {err}", dest.display())))?;
            Ok(Fetched::Written {
                bytes: u64::try_from(body.len()).unwrap_or(u64::MAX),
                validators: Validators {
                    etag: index.then(|| ETAG.to_owned()),
                    last_modified: None,
                },
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use uniwrt_feeds::{Fetched, Source as _, Validators, index};
    use uniwrt_model::facts::PackageManager;

    use super::DemoFeeds;

    #[tokio::test]
    async fn the_demo_s_base_feed_lists_packages_that_match_it() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("index");
        let base = "https://downloads.openwrt.org/releases/24.10.8/packages/x86_64/base";
        let none = Validators::default();
        let fetched = DemoFeeds
            .fetch(&format!("{base}/Packages.gz"), &none, &dest, u64::MAX)
            .await
            .unwrap();
        let Fetched::Written { validators, .. } = fetched else {
            panic!("{fetched:?}");
        };
        let again = DemoFeeds
            .fetch(&format!("{base}/Packages.gz"), &validators, &dest, u64::MAX)
            .await
            .unwrap();
        assert_eq!(again, Fetched::NotModified);
        let read = index::parse(PackageManager::Opkg, &std::fs::read(&dest).unwrap()).unwrap();
        let names: BTreeSet<&str> = read.entries.iter().map(|e| e.name.as_str()).collect();
        for name in ["sqm-scripts", "tcpdump-mini", "kmod-batman-adv", "libc"] {
            assert!(names.contains(name), "{name}");
        }
        let entry = &read.entries[0];
        let file = dir.path().join("package");
        DemoFeeds
            .fetch(&format!("{base}/{}", entry.file), &none, &file, u64::MAX)
            .await
            .unwrap();
        let body = std::fs::read(&file).unwrap();
        assert_eq!(u64::try_from(body.len()).unwrap(), entry.size);
        assert_eq!(uniwrt_feeds::verify::check(entry, &file), Ok(()));
        let adb = format!("{base}/packages.adb");
        assert!(DemoFeeds.fetch(&adb, &none, &file, u64::MAX).await.is_err());
    }
}
