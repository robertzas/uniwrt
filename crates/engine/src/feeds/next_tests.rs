//! The releases nodes are offered: their feed sets and images in the plan, the plan held until
//! every node was looked up, and the lookups' pace.

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use tokio::sync::Semaphore;
use uniwrt_core::NodeId;
use uniwrt_feeds::{BoxFuture, Feed};
use uniwrt_model::{
    facts::{NodeFacts, PackageManager},
    feeds::NextRelease,
    firmware::FirmwareImage,
    inventory::Inventory,
};

use super::{CORE, Harness, facts, inventory, package};
use crate::feeds::{NextReleases, NextSource, plan};

const KMODS: &str = "https://downloads.openwrt.org/releases/24.10.8/targets/x86/64/kmods/6.6.144-1-50daf8372d971124fb3519e8d87e02ae";
const BASE_8: &str = "https://downloads.openwrt.org/releases/24.10.8/packages/x86_64/base";

fn image(release: &str) -> FirmwareImage {
    let name = format!("openwrt-{release}-x86-64-generic-squashfs-combined-efi.img.gz");
    FirmwareImage {
        url: format!("https://downloads.openwrt.org/releases/{release}/targets/x86/64/{name}"),
        name,
        kind: "combined-efi".to_owned(),
        filesystem: Some("squashfs".to_owned()),
        size_bytes: Some(12),
        sha256: "a".repeat(64),
    }
}

fn offered(release: &str, manager: PackageManager, packages: &[&str]) -> NextRelease {
    NextRelease {
        release: release.to_owned(),
        target: "x86/64".to_owned(),
        arch: "x86_64".to_owned(),
        manager: Some(manager),
        kmods: Some("6.12.112-1-b4125196ecec75391093e1e2edeba490".to_owned()),
        packages: packages.iter().map(|&name| name.to_owned()).collect(),
        images: vec![image(release)],
    }
}

#[test]
fn an_offered_release_is_planned_with_derived_feeds_its_reinstall_and_its_image() {
    let opkg = Some(PackageManager::Opkg);
    let old = facts("24.10.8", opkg);
    let old_inv = inventory(
        &[BASE_8, KMODS, "https://example.com/mine"],
        vec![package("luci", true)],
    );
    let next = [offered(
        "25.12.6",
        PackageManager::Apk,
        &["luci", "my-tool", "-ppp"],
    )];
    // Another node runs 25.12.6 already: its set is run, not only offered.
    let new = facts("25.12.6", Some(PackageManager::Apk));
    let new_inv = inventory(
        &["https://downloads.openwrt.org/releases/25.12.6/packages/x86_64/base/packages.adb"],
        Vec::new(),
    );
    let alone = plan([(&old, &old_inv, &next[..])], false);
    let both = plan(
        [(&old, &old_inv, &next[..]), (&new, &new_inv, &next[..0])],
        false,
    );

    let [current, upcoming] = alone.sets.as_slice() else {
        panic!("two sets: {alone:#?}");
    };
    assert_eq!(
        (current.set.release.as_str(), current.offered),
        ("24.10.8", false)
    );
    assert_eq!(
        (
            upcoming.set.release.as_str(),
            upcoming.set.manager,
            upcoming.offered
        ),
        ("25.12.6", PackageManager::Apk, true)
    );
    let urls: Vec<String> = upcoming.feeds.iter().map(Feed::url).collect();
    assert_eq!(
        urls,
        [
            "https://downloads.openwrt.org/releases/25.12.6/packages/x86_64/base",
            "https://downloads.openwrt.org/releases/25.12.6/targets/x86/64/kmods/6.12.112-1-b4125196ecec75391093e1e2edeba490",
        ],
        "the user's own feed doesn't name the release"
    );
    for name in ["luci", "my-tool", "kmod-batman-adv", "stubby"] {
        assert!(upcoming.wanted.contains(name), "{name}");
    }
    assert!(!upcoming.wanted.contains("-ppp") && !upcoming.wanted.contains("ppp"));
    assert_eq!(alone.images, [image("25.12.6")]);

    let [_, run] = both.sets.as_slice() else {
        panic!("two sets: {both:#?}");
    };
    assert_eq!(run.set.release, "25.12.6");
    assert!(!run.offered, "a node runs it");
    assert_eq!(run.feeds.len(), 2, "the node's own and the derived kmods");
    assert_eq!(both.images.len(), 1);
}

/// Answers every lookup with 24.10.5 once let through; counts them.
struct Offers {
    gate: Semaphore,
    asked: AtomicUsize,
}

impl NextReleases for Offers {
    fn next<'a>(
        &'a self,
        _node: NodeId,
        facts: &'a NodeFacts,
        inventory: &'a Inventory,
    ) -> BoxFuture<'a, Result<Vec<NextRelease>, String>> {
        Box::pin(async move {
            self.asked.fetch_add(1, Ordering::SeqCst);
            let _through = self.gate.acquire().await.map_err(|err| err.to_string())?;
            assert_eq!(facts.board.release.version, "24.10.4");
            assert_eq!(inventory.packages.len(), 1);
            Ok(vec![offered("24.10.5", PackageManager::Opkg, &["tcpdump"])])
        })
    }
}

#[tokio::test]
async fn nothing_is_planned_before_the_lookup_and_it_comes_again_hourly() {
    let offers = Arc::new(Offers {
        gate: Semaphore::new(0),
        asked: AtomicUsize::new(0),
    });
    let h = Harness::start_with(Some(&[CORE]), Some(NextSource(offers.clone()))).await;
    h.until("the lookup", |_| offers.asked.load(Ordering::SeqCst) == 1)
        .await;
    tokio::time::sleep(super::QUIET).await;
    let status = h.sync.handle().status();
    assert!(
        status.sets.is_empty() && status.synced_at.is_none(),
        "held: {status:#?}"
    );

    offers.gate.add_permits(100);
    h.until("both sets are planned", |h| {
        h.sync.handle().status().sets.len() == 2
    })
    .await;
    h.settled().await;
    let status = h.sync.handle().status();
    let releases: Vec<(&str, bool)> = status
        .sets
        .iter()
        .map(|set| (set.set.release.as_str(), set.offered))
        .collect();
    assert_eq!(releases, [("24.10.4", false), ("24.10.5", true)]);
    assert_eq!(
        status.sets[1].feeds[0].url,
        "https://downloads.openwrt.org/releases/24.10.5/targets/x86/64/packages"
    );
    let [kept] = status.images.as_slice() else {
        panic!("{status:#?}");
    };
    assert!(
        !kept.kept && kept.error.is_some(),
        "upstream is down: {kept:?}"
    );

    h.clock.advance(Duration::from_mins(59));
    tokio::time::sleep(super::QUIET).await;
    assert_eq!(
        offers.asked.load(Ordering::SeqCst),
        1,
        "not within the hour"
    );
    h.clock.advance(Duration::from_mins(2));
    h.until("the hourly lookup", |_| {
        offers.asked.load(Ordering::SeqCst) == 2
    })
    .await;
    h.sync.stop().await;
}
