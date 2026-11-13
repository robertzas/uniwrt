//! The ASU client against the fake on loopback: the catalog, a build from request to images,
//! and what it refuses before asking.

use uniwrt_model::firmware::ImageBuildState;

use crate::{
    Asu, Error, ImageRequest,
    testing::{FakeAsu, HASH, SHA256},
};

const SCRIPT: &str = "#!/bin/sh\necho first boot\n";

fn request<'a>(release: &'a str, target: &'a str, profile: &'a str) -> ImageRequest<'a> {
    ImageRequest {
        release,
        target,
        profile,
        script: SCRIPT,
    }
}

#[tokio::test]
async fn the_catalog_lists_supported_releases_and_their_devices() {
    let fake = FakeAsu::start().await;
    let asu = Asu::new(fake.url(), "uniwrt/test").unwrap();
    assert_eq!(
        asu.releases().await.unwrap().releases,
        ["25.12.6", "24.10.8"]
    );
    let profiles = asu.profiles("25.12.6").await.unwrap();
    let titles: Vec<&str> = profiles.iter().map(|p| p.titles[0].as_str()).collect();
    assert_eq!(
        titles,
        ["Acme Refused", "Generic x86/64", "GL.iNet GL-MT6000"]
    );
    assert!(matches!(
        asu.profiles("23.05.6").await,
        Err(Error::NotFound(_))
    ));
    assert!(matches!(
        asu.profile("25.12.6", "ath79/generic", "glinet_gl-mt6000")
            .await,
        Err(Error::NotFound(_))
    ));
}

#[tokio::test]
async fn a_build_carries_the_mesh_essentials_and_the_script_then_finishes() {
    let fake = FakeAsu::start().await;
    let asu = Asu::new(&format!("{}/", fake.url()), "uniwrt/test").unwrap();
    let started = asu
        .build(request("25.12.6", "mediatek/filogic", "glinet_gl-mt6000"))
        .await
        .unwrap();
    assert_eq!(
        started.packages,
        [
            "-wpad-basic-mbedtls",
            "wpad-mbedtls",
            "kmod-batman-adv",
            "batctl-default"
        ]
    );
    assert_eq!(started.build.state, ImageBuildState::Queued);
    assert_eq!(started.build.queue_position, Some(2));
    let sent = &fake.builds()[0];
    assert_eq!(sent["version"], "25.12.6");
    assert_eq!(sent["target"], "mediatek/filogic");
    assert_eq!(sent["profile"], "glinet_gl-mt6000");
    assert_eq!(sent["defaults"], SCRIPT);
    assert_eq!(sent["client"], "uniwrt/test");
    assert!(sent.get("diff_packages").is_none(), "the defaults stay");

    let building = asu.status(HASH).await.unwrap();
    assert_eq!(building.state, ImageBuildState::Building);
    let done = asu.status(HASH).await.unwrap();
    assert_eq!(done.state, ImageBuildState::Done);
    assert_eq!(done.images[0].kind, "factory");
    assert_eq!(done.images[0].sha256, SHA256);
    assert_eq!(
        done.images[0].url,
        format!("{}/store/{HASH}/openwrt-squashfs-factory.bin", fake.url())
    );
}

#[tokio::test]
async fn a_device_without_wifi_gets_only_the_mesh() {
    let fake = FakeAsu::start().await;
    let asu = Asu::new(fake.url(), "uniwrt/test").unwrap();
    let started = asu
        .build(request("24.10.8", "x86/64", "generic"))
        .await
        .unwrap();
    assert_eq!(started.packages, ["kmod-batman-adv", "batctl-default"]);
}

#[tokio::test]
async fn refusals_and_unknown_builds() {
    let fake = FakeAsu::start().await;
    let asu = Asu::new(fake.url(), "uniwrt/test").unwrap();
    let refused = asu
        .build(request("25.12.6", "ath79/generic", "refused_one"))
        .await;
    assert!(matches!(refused, Err(Error::Refused(why)) if why.contains("Unsupported profile")));
    let unknown = asu.build(request("25.12.6", "x86/64", "nosuch")).await;
    assert!(matches!(unknown, Err(Error::NotFound(_))));
    let long = "x".repeat(40961);
    let too_long = asu
        .build(ImageRequest {
            script: &long,
            ..request("25.12.6", "x86/64", "generic")
        })
        .await;
    assert!(matches!(too_long, Err(Error::Refused(_))));
    assert_eq!(fake.builds().len(), 1, "only the refused one reached ASU");
    assert!(matches!(
        asu.status("ffffffffffffffff").await,
        Err(Error::NotFound(_))
    ));
    assert!(matches!(
        asu.status("../../etc").await,
        Err(Error::NotFound(_))
    ));
    assert!(Asu::new("ftp://x", "uniwrt/test").is_err());
}

#[tokio::test]
async fn an_unreachable_asu_says_so() {
    let asu = Asu::new("http://127.0.0.1:9", "uniwrt/test").unwrap();
    assert!(matches!(
        asu.releases().await,
        Err(Error::Unreachable { .. })
    ));
}
