//! The mirror against a fake upstream: an opkg feed made up per test.

use std::{
    collections::{BTreeSet, HashMap},
    fmt::Write as _,
    io::Write as _,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use flate2::{Compression, write::GzEncoder};
use sha2::{Digest as _, Sha256};
use tokio::io::AsyncReadExt as _;
use uniwrt_core::{ManualClock, Timestamp};
use uniwrt_model::{facts::PackageManager, feeds::FeedSet};

use crate::{
    BoxFuture, Error, Feed, Fetched, Mirror, Plan, ServeError, SetPlan, Source, Validators,
    index::hex,
};

const BASE: &str = "https://downloads.openwrt.org/releases/24.10.8/packages/x86_64/base";
const PATH: &str = "downloads.openwrt.org/releases/24.10.8/packages/x86_64/base";

/// Upstream: files by URL, each with an `ETag`; down when told.
#[derive(Default)]
struct Upstream {
    files: Mutex<HashMap<String, (Vec<u8>, String)>>,
    down: AtomicBool,
    asked: Mutex<Vec<String>>,
}

impl Upstream {
    fn put(&self, url: &str, body: &[u8]) {
        let etag = format!("\"{}\"", hex(&Sha256::digest(body)));
        self.files
            .lock()
            .unwrap()
            .insert(url.to_owned(), (body.to_vec(), etag));
    }

    fn asked(&self, file: &str) -> usize {
        let url = format!("{BASE}/{file}");
        self.asked
            .lock()
            .unwrap()
            .iter()
            .filter(|u| **u == url)
            .count()
    }

    /// Publishes a feed of `packages` (name, version, depends, contents); `lie` names a
    /// package whose index entry gives another hash.
    fn publish(&self, packages: &[(&str, &str, &str, &[u8])], lie: Option<&str>) {
        let mut text = String::new();
        for (name, version, depends, body) in packages {
            let file = format!("{name}_{version}_x86_64.ipk");
            let hash = if lie == Some(name) {
                Sha256::digest(b"something else")
            } else {
                Sha256::digest(body)
            };
            let _ = write!(
                text,
                "Package: {name}\nVersion: {version}\nDepends: {depends}\nFilename: {file}\nSize: {}\nSHA256sum: {}\nDescription: {name}\n continued\n\n",
                body.len(),
                hex(&hash)
            );
            self.put(&format!("{BASE}/{file}"), body);
        }
        let mut gz = GzEncoder::new(Vec::new(), Compression::default());
        gz.write_all(text.as_bytes()).unwrap();
        let gz = gz.finish().unwrap();
        self.put(&format!("{BASE}/Packages.sig"), &Sha256::digest(&gz));
        self.put(&format!("{BASE}/Packages.gz"), &gz);
    }
}

impl Source for Upstream {
    fn fetch<'a>(
        &'a self,
        url: &'a str,
        validators: &'a Validators,
        dest: &'a Path,
        max: u64,
    ) -> BoxFuture<'a, Result<Fetched, Error>> {
        Box::pin(async move {
            self.asked.lock().unwrap().push(url.to_owned());
            if self.down.load(Ordering::Relaxed) {
                return Err(Error::Unreachable {
                    url: url.to_owned(),
                    why: "no route to host".to_owned(),
                });
            }
            let found = self.files.lock().unwrap().get(url).cloned();
            let Some((body, etag)) = found else {
                return Err(Error::Status {
                    url: url.to_owned(),
                    status: 404,
                });
            };
            if validators.etag.as_deref() == Some(etag.as_str()) {
                return Ok(Fetched::NotModified);
            }
            if body.len() as u64 > max {
                return Err(Error::TooLarge {
                    url: url.to_owned(),
                    max_mib: max >> 20,
                });
            }
            tokio::fs::write(dest, &body).await.unwrap();
            Ok(Fetched::Written {
                bytes: body.len() as u64,
                validators: Validators {
                    etag: Some(etag),
                    last_modified: None,
                },
            })
        })
    }
}

struct Harness {
    _dir: tempfile::TempDir,
    root: std::path::PathBuf,
    upstream: Arc<Upstream>,
    clock: ManualClock,
    mirror: Mirror,
}

async fn harness() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("feeds");
    let upstream = Arc::new(Upstream::default());
    let clock = ManualClock::new(Timestamp::from_unix_secs(1_800_000_000));
    let mirror = Mirror::open(root.clone(), upstream.clone(), Arc::new(clock.clone()))
        .await
        .unwrap();
    Harness {
        _dir: dir,
        root,
        upstream,
        clock,
        mirror,
    }
}

fn set() -> FeedSet {
    FeedSet {
        release: "24.10.8".to_owned(),
        target: "x86/64".to_owned(),
        arch: "x86_64".to_owned(),
        manager: PackageManager::Opkg,
    }
}

fn plan(wanted: &[&str]) -> Plan {
    Plan {
        sets: vec![SetPlan {
            set: set(),
            feeds: vec![Feed::parse(BASE).unwrap()],
            wanted: wanted.iter().map(|&name| name.to_owned()).collect(),
            offered: false,
        }],
        whole: false,
        images: Vec::new(),
    }
}

fn kept(root: &Path) -> BTreeSet<String> {
    std::fs::read_dir(root.join(PATH)).map_or_else(
        |_| BTreeSet::new(),
        |read| {
            read.flatten()
                .map(|item| item.file_name().to_string_lossy().into_owned())
                .collect()
        },
    )
}

async fn body(mirror: &Mirror, file: &str) -> Result<Vec<u8>, ServeError> {
    let mut served = mirror.serve(&format!("{PATH}/{file}")).await?;
    let mut out = Vec::new();
    served.file.read_to_end(&mut out).await.unwrap();
    assert_eq!(out.len() as u64, served.len);
    Ok(out)
}

const FEED: &[(&str, &str, &str, &[u8])] = &[
    ("a", "1", "libc, b (>= 1)", b"package a"),
    ("b", "1", "libc", b"package b"),
    ("c", "1", "libc", b"package c"),
];

#[tokio::test]
async fn an_apk_feed_round_trips_through_the_mirror() {
    let h = harness().await;
    let apk = "https://downloads.openwrt.org/releases/25.12.6/packages/x86_64/base";
    let mut entries = Vec::new();
    for (name, depends, data) in [("a", "b", &b"package a"[..]), ("b", "", b"package b")] {
        let (file, hash) = crate::index::adb::package(name, "1-r1", data).unwrap();
        entries.push(crate::index::Entry {
            name: name.to_owned(),
            version: "1-r1".to_owned(),
            file: format!("{name}-1-r1.apk"),
            size: file.len() as u64,
            check: Some(crate::index::Check::ApkControl(hash)),
            depends: (!depends.is_empty())
                .then(|| vec![depends.to_owned()])
                .into_iter()
                .collect(),
            provides: Vec::new(),
        });
        h.upstream.put(&format!("{apk}/{name}-1-r1.apk"), &file);
    }
    let index = crate::index::Index { entries };
    h.upstream.put(
        &format!("{apk}/packages.adb"),
        &crate::index::adb::write(&index).unwrap(),
    );
    let mut plan = plan(&["a"]);
    plan.sets[0].set.release = "25.12.6".to_owned();
    plan.sets[0].set.manager = PackageManager::Apk;
    plan.sets[0].feeds = vec![Feed::parse(&format!("{apk}/packages.adb")).unwrap()];
    h.mirror.sync(plan, true).await.unwrap();
    let status = h.mirror.status();
    assert!(status.sync_error.is_none(), "{status:?}");
    assert_eq!(
        (status.sets[0].kept, status.sets[0].feeds[0].listed),
        (2, 2)
    );
    let path = "downloads.openwrt.org/releases/25.12.6/packages/x86_64/base";
    let mut served = h.mirror.serve(&format!("{path}/a-1-r1.apk")).await.unwrap();
    let mut out = Vec::new();
    served.file.read_to_end(&mut out).await.unwrap();
    assert_eq!(out.len() as u64, index.entries[0].size);
    assert!(
        h.mirror
            .serve(&format!("{path}/packages.adb"))
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn a_sync_keeps_what_is_wanted_with_its_dependencies_and_serves_it() {
    let h = harness().await;
    h.upstream.publish(FEED, None);
    h.mirror.sync(plan(&["a"]), true).await.unwrap();
    assert_eq!(
        kept(&h.root),
        BTreeSet::from(
            [
                "Packages.gz",
                "Packages.sig",
                "a_1_x86_64.ipk",
                "b_1_x86_64.ipk"
            ]
            .map(String::from)
        )
    );
    let status = h.mirror.status();
    assert_eq!(status.sets[0].kept, 2);
    assert_eq!(status.sets[0].feeds[0].listed, 3);
    assert_eq!(status.bytes, 18);
    assert!(status.sync_error.is_none(), "{status:?}");
    assert_eq!(
        body(&h.mirror, "a_1_x86_64.ipk").await.unwrap(),
        b"package a"
    );
    let sig = body(&h.mirror, "Packages.sig").await.unwrap();
    assert_eq!(sig.len(), 32);

    // A package nobody wanted yet comes through, and is wanted from then on.
    assert_eq!(
        body(&h.mirror, "c_1_x86_64.ipk").await.unwrap(),
        b"package c"
    );
    assert_eq!(h.mirror.status().sets[0].kept, 3);

    // A new index: the newest versions follow, the old ones go.
    h.upstream.publish(
        &[
            ("a", "2", "libc, b", b"package a, 2"),
            ("b", "1", "libc", b"package b"),
            ("c", "2", "libc", b"package c, 2"),
        ],
        None,
    );
    h.mirror.sync(plan(&["a"]), true).await.unwrap();
    assert_eq!(
        kept(&h.root),
        BTreeSet::from(
            [
                "Packages.gz",
                "Packages.sig",
                "a_2_x86_64.ipk",
                "b_1_x86_64.ipk",
                "c_2_x86_64.ipk"
            ]
            .map(String::from)
        )
    );
    // Unchanged files weren't downloaded again.
    assert_eq!(h.upstream.asked("b_1_x86_64.ipk"), 1);
}

#[tokio::test]
async fn a_package_that_doesn_t_match_its_index_is_never_kept() {
    let h = harness().await;
    h.upstream.publish(FEED, Some("b"));
    h.mirror.sync(plan(&["a"]), true).await.unwrap();
    assert!(!kept(&h.root).contains("b_1_x86_64.ipk"));
    let status = h.mirror.status();
    assert!(status.sets[0].failed[0].contains("b 1"), "{status:?}");
    assert!(status.sync_error.unwrap().contains("couldn't be had"));
    assert!(matches!(
        body(&h.mirror, "b_1_x86_64.ipk").await,
        Err(ServeError::Unavailable(why)) if why.contains("hash")
    ));
    // Nothing half-written stays behind.
    assert_eq!(
        std::fs::read_dir(h.root.join(".partial")).unwrap().count(),
        0
    );
}

#[tokio::test]
async fn offline_the_copies_kept_are_served_and_the_failure_is_shown() {
    let h = harness().await;
    h.upstream.publish(FEED, None);
    h.mirror.sync(plan(&["a"]), true).await.unwrap();
    h.upstream.down.store(true, Ordering::Relaxed);
    h.clock.advance(Duration::from_hours(13));
    h.mirror.sync(plan(&["a"]), true).await.unwrap();
    let status = h.mirror.status();
    assert!(
        status.sets[0].feeds[0]
            .error
            .as_deref()
            .unwrap()
            .contains("no route")
    );
    assert!(status.sets[0].feeds[0].index_at.is_some());
    assert_eq!(status.sets[0].kept, 2);
    assert!(body(&h.mirror, "Packages.gz").await.is_ok());
    assert_eq!(
        body(&h.mirror, "b_1_x86_64.ipk").await.unwrap(),
        b"package b"
    );
    assert!(matches!(
        body(&h.mirror, "c_1_x86_64.ipk").await,
        Err(ServeError::Unavailable(_))
    ));
}

#[tokio::test]
async fn an_index_over_an_hour_old_is_asked_for_again_when_a_node_asks() {
    let h = harness().await;
    h.upstream.publish(FEED, None);
    h.mirror.sync(plan(&["a"]), true).await.unwrap();
    let first = body(&h.mirror, "Packages.gz").await.unwrap();
    h.upstream.publish(&FEED[..2], None);
    h.clock.advance(Duration::from_mins(10));
    assert_eq!(body(&h.mirror, "Packages.gz").await.unwrap(), first);
    h.clock.advance(Duration::from_hours(1));
    assert_ne!(body(&h.mirror, "Packages.gz").await.unwrap(), first);
    assert!(matches!(
        body(&h.mirror, "c_1_x86_64.ipk").await,
        Err(ServeError::NotFound)
    ));
}

#[tokio::test]
async fn only_planned_feeds_and_listed_files_are_served() {
    let h = harness().await;
    h.upstream.publish(FEED, None);
    assert!(matches!(
        h.mirror.serve(&format!("{PATH}/Packages.gz")).await,
        Err(ServeError::NotFound)
    ));
    h.mirror.sync(plan(&[]), true).await.unwrap();
    for path in [
        format!("{PATH}/../../../../etc/passwd"),
        format!("{PATH}/nope.ipk"),
        format!("{PATH}/.."),
        "downloads.openwrt.org/releases/24.10.8/packages/x86_64/luci/Packages.gz".to_owned(),
        ".state.json".to_owned(),
        String::new(),
    ] {
        assert!(
            matches!(h.mirror.serve(&path).await, Err(ServeError::NotFound)),
            "{path}"
        );
    }
}

#[tokio::test]
async fn whole_feeds_keep_everything_and_an_empty_plan_keeps_nothing() {
    let h = harness().await;
    h.upstream.publish(FEED, None);
    let mut whole = plan(&[]);
    whole.whole = true;
    h.mirror.sync(whole, true).await.unwrap();
    assert_eq!(h.mirror.status().sets[0].kept, 3);
    h.mirror.sync(Plan::default(), true).await.unwrap();
    let left: BTreeSet<String> = std::fs::read_dir(&h.root)
        .unwrap()
        .flatten()
        .map(|item| item.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        left,
        BTreeSet::from([".partial", ".state.json"].map(String::from))
    );

    // The state survives a restart.
    let again = Mirror::open(
        h.root.clone(),
        h.upstream.clone(),
        Arc::new(h.clock.clone()),
    )
    .await
    .unwrap();
    assert!(again.inner.state().synced.is_some());
}

#[path = "images_tests.rs"]
mod images;
