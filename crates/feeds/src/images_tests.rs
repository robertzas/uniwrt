//! The official images kept for upgrades on offer: downloaded once, kept only when they match
//! their release's SHA-256 and size, found by the upgrade, and swept once no plan names them.

use sha2::{Digest as _, Sha256};
use uniwrt_model::firmware::FirmwareImage;

use super::{FEED, harness, plan};
use crate::index::hex;

const DIR: &str = "https://downloads.openwrt.org/releases/25.12.6/targets/x86/64";

fn image(name: &str, listed: &[u8]) -> FirmwareImage {
    FirmwareImage {
        name: name.to_owned(),
        kind: "combined-efi".to_owned(),
        filesystem: Some("squashfs".to_owned()),
        size_bytes: Some(listed.len() as u64),
        sha256: hex(&Sha256::digest(listed)),
        url: format!("{DIR}/{name}"),
    }
}

#[tokio::test]
async fn an_image_is_kept_once_it_matches_found_by_the_upgrade_and_swept_when_unplanned() {
    let h = harness().await;
    h.upstream.publish(FEED, None);
    let body = b"an x86 image".as_slice();
    let good = image(
        "openwrt-25.12.6-x86-64-generic-squashfs-combined-efi.img.gz",
        body,
    );
    let liar = image(
        "openwrt-25.12.6-x86-64-generic-ext4-combined-efi.img.gz",
        b"other bytes!",
    );
    h.upstream.put(&good.url, body);
    h.upstream.put(&liar.url, body);
    let mut with = plan(&[]);
    with.images = vec![good.clone(), liar.clone()];
    h.mirror.sync(with.clone(), true).await.unwrap();

    let path = h.mirror.image(&good).expect("kept");
    assert_eq!(std::fs::read(&path).unwrap(), body);
    assert!(
        path.starts_with(
            h.root
                .join("downloads.openwrt.org/releases/25.12.6/targets/x86/64")
        )
    );
    assert_eq!(h.mirror.image(&liar), None);
    let mut other = good.clone();
    other.sha256 = liar.sha256.clone();
    assert_eq!(h.mirror.image(&other), None, "only the planned SHA-256");
    let status = h.mirror.status();
    let seen: Vec<(bool, bool)> = status
        .images
        .iter()
        .map(|image| (image.kept, image.error.is_some()))
        .collect();
    assert_eq!(seen, [(true, false), (false, true)]);
    assert!(status.sync_error.unwrap().contains("ext4"));

    // Kept: offline, it isn't asked for again.
    h.upstream
        .down
        .store(true, std::sync::atomic::Ordering::Relaxed);
    let asked = |url: &str| {
        h.upstream
            .asked
            .lock()
            .unwrap()
            .iter()
            .filter(|u| *u == url)
            .count()
    };
    let before = asked(&good.url);
    h.mirror.sync(with, false).await.unwrap();
    assert_eq!(asked(&good.url), before);
    assert!(h.mirror.image(&good).is_some());

    h.mirror.sync(plan(&[]), false).await.unwrap();
    assert_eq!(h.mirror.image(&good), None);
    assert!(!path.exists(), "swept once no plan names it");
}
