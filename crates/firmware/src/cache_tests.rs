use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use axum::{
    Router,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
};
use uniwrt_model::firmware::FirmwareImage;

use super::*;
use crate::{
    GitHub,
    testing::{FakeGitHub, PRIVATE_ASSET, TOKEN},
};

const BODY: &[u8] = b"an OpenWrt sysupgrade image";

fn sha256(data: &[u8]) -> String {
    HEXLOWER.encode(ring::digest::digest(&ring::digest::SHA256, data).as_ref())
}

/// A download server: `/img` the image (counted), `/moved` a redirect to it, `/other` other bytes.
async fn server() -> (String, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let hits = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&hits);
    let target = format!("{url}/img");
    let app = Router::new()
        .route(
            "/img",
            get(move |_: HeaderMap| {
                counted.fetch_add(1, Ordering::SeqCst);
                async { BODY.into_response() }
            }),
        )
        .route(
            "/moved",
            get(move || async move { (StatusCode::FOUND, [("location", target)]).into_response() }),
        )
        .route(
            "/other",
            get(|| async { b"something else".into_response() }),
        );
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (url, hits, task)
}

fn image(name: &str, url: String, data: &[u8]) -> FirmwareImage {
    FirmwareImage {
        name: name.to_owned(),
        kind: "sysupgrade".to_owned(),
        filesystem: Some("squashfs".to_owned()),
        size_bytes: Some(data.len() as u64),
        sha256: sha256(data),
        url,
    }
}

#[tokio::test]
async fn an_image_is_downloaded_once_checked_and_reused() {
    let (url, hits, task) = server().await;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("left.part"), b"unfinished").unwrap();
    let cache = ImageCache::open(dir.path().to_owned(), "uniwrt/test")
        .await
        .unwrap();
    assert!(
        !dir.path().join("left.part").exists(),
        "a stop's leftover goes"
    );

    let wanted = image("a.bin", format!("{url}/img"), BODY);
    let path = cache.fetch(&wanted).await.unwrap();
    assert_eq!(path, dir.path().join(format!("{}.img", wanted.sha256)));
    assert_eq!(std::fs::read(&path).unwrap(), BODY);
    assert_eq!(cache.fetch(&wanted).await.unwrap(), path);
    assert_eq!(hits.load(Ordering::SeqCst), 1, "kept, not downloaded again");

    // Redirects are followed.
    std::fs::remove_file(&path).unwrap();
    let moved = image("a.bin", format!("{url}/moved"), BODY);
    assert_eq!(cache.fetch(&moved).await.unwrap(), path);
    assert_eq!(hits.load(Ordering::SeqCst), 2);
    task.abort();
}

#[tokio::test]
async fn an_image_that_isnt_what_its_source_lists_is_refused() {
    let (url, _, task) = server().await;
    let dir = tempfile::tempdir().unwrap();
    let cache = ImageCache::open(dir.path().to_owned(), "uniwrt/test")
        .await
        .unwrap();
    let mut wrong = image("a.bin", format!("{url}/other"), BODY);
    wrong.size_bytes = None;
    let err = cache.fetch(&wrong).await.unwrap_err();
    assert!(matches!(err, Error::Untrusted(_)), "{err:?}");
    let left: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
    assert!(left.is_empty(), "nothing is kept: {left:?}");

    let mut bad = image("a.bin", format!("{url}/img"), BODY);
    bad.sha256 = "../etc".to_owned();
    assert!(matches!(
        cache.fetch(&bad).await.unwrap_err(),
        Error::Invalid(_)
    ));
    task.abort();
}

#[tokio::test]
async fn only_the_images_used_last_are_kept() {
    let dir = tempfile::tempdir().unwrap();
    let cache = ImageCache::open(dir.path().to_owned(), "uniwrt/test")
        .await
        .unwrap();
    let mut images = Vec::new();
    for i in 0..=MAX_CACHED {
        let data = format!("image {i}").into_bytes();
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let url = format!("http://{}/img", listener.local_addr().unwrap());
        let body = data.clone();
        let app = Router::new().route("/img", get(move || async move { body.into_response() }));
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let wanted = image(&format!("{i}.bin"), url, &data);
        let path = cache.fetch(&wanted).await.unwrap();
        task.abort();
        // Distinct times: the first is used again before the last arrives.
        let at = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000 + i as u64);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(at)
            .unwrap();
        images.push((wanted, path));
        if i == MAX_CACHED - 1 {
            cache.fetch(&images[0].0).await.unwrap();
        }
    }
    let kept: Vec<bool> = images.iter().map(|(_, path)| path.exists()).collect();
    assert_eq!(
        kept,
        [true, false, true, true, true],
        "the second was used least lately"
    );
}

#[tokio::test]
async fn a_private_repos_image_downloads_with_the_token_which_stays_with_the_api() {
    let fake = FakeGitHub::start().await;
    let github = GitHub::new(fake.url(), "uniwrt/test").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let cache = ImageCache::open(dir.path().to_owned(), "uniwrt/test")
        .await
        .unwrap();
    let name = "openwrt-25.12.6-mediatek-filogic-glinet_gl-mt6000-squashfs-sysupgrade.bin";
    let path = github
        .fetch_image(&cache, "owner/private", "p1", name, Some(TOKEN))
        .await
        .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), PRIVATE_ASSET);
    assert_eq!(
        fake.storage_tokens(),
        [(name.to_owned(), None)],
        "the token isn't sent on to the storage"
    );
    assert!(matches!(
        github
            .fetch_image(&cache, "owner/private", "p1", "missing.bin", Some(TOKEN))
            .await
            .unwrap_err(),
        Error::NotFound(_)
    ));
}
