//! The next releases against the fake ASU, and the catalog kept on disk serving offline.

use uniwrt_core::{NodeId, Timestamp};
use uniwrt_model::{
    facts::{PackageManager, Release},
    inventory::Package,
};

use crate::{
    Asu, Error, Upgrade,
    testing::{FakeAsu, KERNEL_HASH},
};

const BOARD: &str = "glinet,gl-mt6000";

fn release(version: &str) -> Release {
    Release {
        distribution: "OpenWrt".to_owned(),
        version: version.to_owned(),
        revision: "r1-test".to_owned(),
        target: "mediatek/filogic".to_owned(),
        arch: "aarch64_cortex-a53".to_owned(),
    }
}

fn packages() -> Vec<Package> {
    ["opkg", "luci", "wpad-basic-mbedtls", "my-tool"]
        .into_iter()
        .map(|name| Package {
            name: name.to_owned(),
            version: "1".to_owned(),
            explicit: name != "wpad-basic-mbedtls",
        })
        .collect()
}

async fn next(asu: &Asu, board: &str) -> Result<Vec<uniwrt_model::feeds::NextRelease>, Error> {
    asu.next_releases(
        NodeId::new(3),
        &release("24.10.7"),
        board,
        &packages(),
        Timestamp::from_unix_ms(1_000),
    )
    .await
}

#[tokio::test]
async fn each_offer_comes_with_its_feeds_parts_packages_and_official_images() {
    let fake = FakeAsu::start().await;
    let asu = Asu::new(fake.url(), "uniwrt/test").unwrap();
    let offered = next(&asu, BOARD).await.unwrap();
    let seen: Vec<(&str, Option<PackageManager>, Option<&str>)> = offered
        .iter()
        .map(|n| (n.release.as_str(), n.manager, n.kmods.as_deref()))
        .collect();
    let kmods = |version: &str| format!("{version}-1-{KERNEL_HASH}");
    assert_eq!(
        seen,
        [
            (
                "25.12.6",
                Some(PackageManager::Apk),
                Some(kmods("6.12.112").as_str())
            ),
            (
                "24.10.8",
                Some(PackageManager::Opkg),
                Some(kmods("6.6.119").as_str())
            ),
        ]
    );
    let running = release("24.10.7");
    let packages = packages();
    for offered in &offered {
        assert_eq!(
            (offered.target.as_str(), offered.arch.as_str()),
            ("mediatek/filogic", "aarch64_cortex-a53")
        );
        let plan = asu
            .official_plan(Upgrade {
                node: NodeId::new(3),
                release: &running,
                board: BOARD,
                to: &offered.release,
                packages: &packages,
                packages_at: Timestamp::from_unix_ms(1_000),
                tmp_free_bytes: None,
            })
            .await
            .unwrap();
        assert_eq!(
            offered.packages, plan.packages,
            "what the reinstall asks for"
        );
        assert_eq!(offered.images, plan.images);
        assert_eq!(offered.images.len(), 1, "the sysupgrade image");
    }
    assert_eq!(
        offered[0].packages.iter().filter(|p| *p == "luci").count(),
        1
    );
    assert!(
        next(&asu, "acme,other").await.unwrap().is_empty(),
        "no image"
    );
}

#[tokio::test]
async fn offline_the_catalog_kept_last_answers() {
    let dir = tempfile::tempdir().unwrap();
    let fake = FakeAsu::start().await;
    let url = fake.url().to_owned();
    let online = Asu::new(&url, "uniwrt/test")
        .unwrap()
        .keeping_catalog_in(dir.path().join("catalog"));
    let before = next(&online, BOARD).await.unwrap();
    drop(fake);
    // A new controller (nothing in memory), with ASU gone.
    let offline = Asu::new(&url, "uniwrt/test")
        .unwrap()
        .keeping_catalog_in(dir.path().join("catalog"));
    assert_eq!(next(&offline, BOARD).await.unwrap(), before);
    let forgetful = Asu::new(&url, "uniwrt/test").unwrap();
    assert!(matches!(
        next(&forgetful, BOARD).await,
        Err(Error::Unreachable { .. })
    ));
    // Builds aren't the catalog: their status is never answered from disk.
    assert!(offline.status(crate::testing::HASH).await.is_err());
}
