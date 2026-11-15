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

mod upgrades {
    use uniwrt_core::{NodeId, Timestamp};
    use uniwrt_model::{
        facts::Release,
        firmware::{OfferKind, PackageRename},
        inventory::Package,
    };

    use crate::{
        Asu, Error, Upgrade,
        testing::{FakeAsu, SYSUPGRADE_BYTES},
    };

    fn release(version: &str) -> Release {
        Release {
            distribution: "OpenWrt".to_owned(),
            version: version.to_owned(),
            revision: "r29233-443ec4032a".to_owned(),
            target: "mediatek/filogic".to_owned(),
            arch: "aarch64_cortex-a53".to_owned(),
        }
    }

    fn package(name: &str, explicit: bool) -> Package {
        Package {
            name: name.to_owned(),
            version: "1".to_owned(),
            explicit,
        }
    }

    /// A 24.10.7 node with LuCI, the full wpad, the mesh and a package from another feed.
    fn packages() -> Vec<Package> {
        let mut packages: Vec<Package> = [
            "opkg",
            "dnsmasq",
            "luci",
            "wpad-mbedtls",
            "kmod-batman-adv",
            "batctl-default",
            "my-tool",
            "kernel",
        ]
        .into_iter()
        .map(|name| package(name, true))
        .collect();
        packages.push(package("ppp", false));
        packages.push(package("kmod-mt7915e", false));
        packages
    }

    fn upgrade<'a>(release: &'a Release, to: &'a str, packages: &'a [Package]) -> Upgrade<'a> {
        Upgrade {
            node: NodeId::new(3),
            release,
            board: "glinet,gl-mt6000",
            to,
            packages,
            packages_at: Timestamp::from_unix_ms(1_000),
            tmp_free_bytes: Some(200 << 20),
        }
    }

    #[tokio::test]
    async fn a_node_is_offered_the_next_branch_and_its_own_latest() {
        let fake = FakeAsu::start().await;
        let asu = Asu::new(fake.url(), "uniwrt/test").unwrap();
        let (offers, note) = asu
            .offers(&release("24.10.7"), "glinet,gl-mt6000")
            .await
            .unwrap();
        assert_eq!(note, None);
        let seen: Vec<(&str, OfferKind, Option<&str>)> = offers
            .iter()
            .map(|o| (o.release.as_str(), o.kind, o.profile.as_deref()))
            .collect();
        assert_eq!(
            seen,
            [
                ("25.12.6", OfferKind::Branch, Some("glinet_gl-mt6000")),
                ("24.10.8", OfferKind::Update, Some("glinet_gl-mt6000")),
            ]
        );
        assert_eq!(offers[0].image_bytes, Some(SYSUPGRADE_BYTES));
        assert!(offers[0].notes.ends_with("/releases/25.12/notes-25.12.6"));

        let (offers, note) = asu.offers(&release("24.10.7"), "acme,other").await.unwrap();
        assert!(offers.iter().all(|o| o.profile.is_none()));
        assert!(note.unwrap().contains("acme,other"));
        let mut derivative = release("24.10.7");
        derivative.distribution = "ImmortalWrt".to_owned();
        let (offers, note) = asu.offers(&derivative, "glinet,gl-mt6000").await.unwrap();
        assert!(offers.is_empty() && note.unwrap().contains("ImmortalWrt"));
        let (offers, _) = asu
            .offers(&release("25.12.6"), "glinet,gl-mt6000")
            .await
            .unwrap();
        assert!(offers.is_empty(), "up to date");
    }

    #[tokio::test]
    async fn an_upgrade_keeps_the_nodes_packages_and_builds_without_a_script() {
        let fake = FakeAsu::start().await;
        let asu = Asu::new(fake.url(), "uniwrt/test").unwrap();
        let running = release("24.10.7");
        let packages = packages();
        let plan = asu
            .upgrade_plan(upgrade(&running, "25.12.6", &packages))
            .await
            .unwrap();
        assert_eq!(plan.profile, "glinet_gl-mt6000");
        assert_eq!(
            plan.kept,
            ["batctl-default", "kmod-batman-adv", "luci", "wpad-mbedtls"]
        );
        assert_eq!(
            plan.renamed,
            [PackageRename {
                from: "opkg".to_owned(),
                to: "apk-mbedtls".to_owned()
            }]
        );
        assert_eq!(plan.dropped, ["my-tool"]);
        assert_eq!(plan.removed, ["wpad-basic-mbedtls"]);
        assert_eq!(plan.image_bytes, Some(SYSUPGRADE_BYTES));

        let started = asu
            .build_upgrade(upgrade(&running, "25.12.6", &packages))
            .await
            .unwrap();
        assert_eq!(started.packages, plan.packages);
        let sent = fake.builds().pop().unwrap();
        assert_eq!(sent["version"], "25.12.6");
        assert_eq!(sent["profile"], "glinet_gl-mt6000");
        assert!(sent.get("defaults").is_none(), "{sent}");
        assert!(
            sent["packages"]
                .as_array()
                .unwrap()
                .contains(&"-wpad-basic-mbedtls".into())
        );
    }

    #[tokio::test]
    async fn an_update_within_a_branch_renames_nothing() {
        let fake = FakeAsu::start().await;
        let asu = Asu::new(fake.url(), "uniwrt/test").unwrap();
        let running = release("24.10.7");
        let packages = packages();
        let plan = asu
            .upgrade_plan(upgrade(&running, "24.10.8", &packages))
            .await
            .unwrap();
        assert_eq!(
            plan.renamed,
            [] as [uniwrt_model::firmware::PackageRename; 0]
        );
        assert!(!plan.packages.iter().any(|p| p.contains("opkg")));
    }

    #[tokio::test]
    async fn only_a_release_on_offer_is_planned() {
        let fake = FakeAsu::start().await;
        let asu = Asu::new(fake.url(), "uniwrt/test").unwrap();
        let running = release("24.10.7");
        let packages = packages();
        for to in ["25.12.5", "24.10.7", "26.06.0", "../x"] {
            assert!(
                matches!(
                    asu.upgrade_plan(upgrade(&running, to, &packages)).await,
                    Err(Error::NotFound(_))
                ),
                "{to}"
            );
        }
        assert_eq!(fake.builds(), [] as [serde_json::Value; 0]);
    }
}
