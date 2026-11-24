//! The mirror's plan from the nodes' facts and inventories, and its task over a store and a
//! source that never answers: a refresh at start, again after a failed one, on demand, a sync
//! for a changed plan, nothing while off, and the feeds of a node whose inventory has none
//! read over its link.

use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

use serde_json::json;
use uniwrt_core::{MacAddr, ManualClock, NodeId, Timestamp};
use uniwrt_feeds::{BoxFuture, Fetched, Mirror, ServeError, Source, Validators};
use uniwrt_model::{
    facts::{NodeFacts, PackageManager},
    feeds::MirrorSettings,
    inventory::{Inventory, NodeFeed, Package},
    node::{NewNode, NodeRole},
};
use uniwrt_store::{FactsRepo as _, InventoryRepo as _, NodeRepo as _, SettingsRepo as _, Store};

use super::{FeedMirrorConfig, FeedMirrorError, FeedMirrorSync, plan};
use crate::{
    Engine, EngineConfig, Identity,
    testing::{self, hello, running},
};

const NOW: Timestamp = Timestamp::from_unix_secs(1_800_000_000);
const QUIET: Duration = Duration::from_millis(300);
const WAIT: Duration = Duration::from_secs(5);
const CORE: &str = "https://downloads.openwrt.org/releases/24.10.4/targets/x86/64/packages";
const BASE: &str = "https://downloads.openwrt.org/releases/24.10.4/packages/x86_64/base";

fn facts(version: &str, manager: Option<PackageManager>) -> NodeFacts {
    let mut facts = NodeFacts::default();
    facts.capabilities.package_manager = manager;
    let release = &mut facts.board.release;
    release.version = version.to_owned();
    release.target = "x86/64".to_owned();
    release.arch = "x86_64".to_owned();
    facts
}

fn package(name: &str, explicit: bool) -> Package {
    Package {
        name: name.to_owned(),
        version: "1".to_owned(),
        explicit,
    }
}

fn inventory(feeds: &[&str], packages: Vec<Package>) -> Inventory {
    Inventory {
        taken_at: NOW,
        packages,
        feeds: feeds
            .iter()
            .map(|url| NodeFeed {
                name: None,
                url: (*url).to_owned(),
            })
            .collect(),
    }
}

#[test]
fn nodes_of_one_release_share_a_feed_set_with_what_uniwrt_installs_and_they_chose() {
    let opkg = Some(PackageManager::Opkg);
    let (a, b) = (facts("24.10.4", opkg), facts("24.10.4", opkg));
    let a_inv = inventory(
        &[CORE, "file:///root/local"],
        vec![
            package("luci", true),
            package("libc", false),
            package("wpad-basic-mbedtls", false),
        ],
    );
    let b_inv = inventory(&[CORE, BASE], vec![package("tcpdump", true)]);
    let (unknown, unknown_inv) = (facts("24.10.4", None), inventory(&[CORE], Vec::new()));
    let (none, none_inv) = (
        facts("23.05.5", opkg),
        inventory(&["file:///x"], Vec::new()),
    );
    let made = plan(
        [
            (&a, &a_inv),
            (&b, &b_inv),
            (&unknown, &unknown_inv),
            (&none, &none_inv),
        ],
        true,
    );
    assert!(made.whole);
    let [set] = made.sets.as_slice() else {
        panic!("one set: {made:#?}");
    };
    assert_eq!(
        (
            set.set.release.as_str(),
            set.set.target.as_str(),
            set.set.arch.as_str()
        ),
        ("24.10.4", "x86/64", "x86_64")
    );
    let urls: Vec<String> = set.feeds.iter().map(uniwrt_feeds::Feed::url).collect();
    assert_eq!(urls, [CORE, BASE], "each feed once, the local one left out");
    for name in [
        "luci",
        "tcpdump",
        "wpad-mbedtls",
        "sqm-scripts",
        "stubby",
        "tcpdump-mini",
        "ethtool-full",
        "kmod-batman-adv",
        "batctl-default",
    ] {
        assert!(set.wanted.contains(name), "{name}: {:?}", set.wanted);
    }
    assert!(
        !set.wanted.contains("libc"),
        "dependencies come from the index"
    );
}

/// Every `Requirement` in render's source, by its package name.
fn requirements(dir: &Path, found: &mut std::collections::BTreeSet<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            requirements(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let text = std::fs::read_to_string(&path).unwrap();
            for rest in text.split("package: \"").skip(1) {
                if let Some((name, after)) = rest.split_once('"')
                    && after.starts_with(',')
                {
                    found.insert(name.to_owned());
                }
            }
        }
    }
}

#[test]
fn the_mirror_wants_every_package_render_may_require() {
    let mut found = std::collections::BTreeSet::new();
    let render = Path::new(env!("CARGO_MANIFEST_DIR")).join("../render/src");
    requirements(&render, &mut found);
    let listed: std::collections::BTreeSet<String> = uniwrt_render::every_package()
        .into_iter()
        .map(str::to_owned)
        .collect();
    assert!(found.len() >= 16, "{found:?}");
    assert_eq!(
        listed, found,
        "render::every_package() names each Requirement"
    );
}

/// Every index request fails; the URLs asked for are kept.
#[derive(Default)]
struct Unreachable(Mutex<Vec<String>>);

impl Unreachable {
    fn asked(&self) -> usize {
        self.0.lock().unwrap().len()
    }
}

impl Source for Unreachable {
    fn fetch<'a>(
        &'a self,
        url: &'a str,
        _validators: &'a Validators,
        _dest: &'a Path,
        _max: u64,
    ) -> BoxFuture<'a, Result<Fetched, uniwrt_feeds::Error>> {
        self.0.lock().unwrap().push(url.to_owned());
        Box::pin(async move {
            Err(uniwrt_feeds::Error::Unreachable {
                url: url.to_owned(),
                why: "offline".to_owned(),
            })
        })
    }
}

struct Harness {
    _dir: tempfile::TempDir,
    clock: Arc<ManualClock>,
    store: Store,
    engine: Engine<testing::SharedLink>,
    sync: FeedMirrorSync,
    source: Arc<Unreachable>,
    node: NodeId,
}

impl Harness {
    /// With an inventory naming `feeds`, or none stored.
    async fn start(feeds: Option<&[&str]>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(ManualClock::new(NOW));
        let store = Store::open(dir.path().join("test.db"), clock.clone())
            .await
            .unwrap();
        let node = store
            .create_node(NewNode {
                name: "ap".to_owned(),
                mac: MacAddr::new([2, 0, 0, 0, 0, 2]),
                board_name: "x".to_owned(),
                host: "10.0.0.2".to_owned(),
                port: 22,
                role: NodeRole::Ap,
            })
            .await
            .unwrap()
            .id;
        let stored = facts("24.10.4", Some(PackageManager::Opkg));
        store.save_facts(node, &stored).await.unwrap();
        if let Some(feeds) = feeds {
            let packages = vec![package("luci", true)];
            store
                .set_inventory(node, inventory(feeds, packages))
                .await
                .unwrap();
        }
        let source = Arc::new(Unreachable::default());
        let mirror = Mirror::open(dir.path().join("feeds"), source.clone(), clock.clone())
            .await
            .unwrap();
        let engine = Engine::new(EngineConfig::default(), clock.clone());
        let config = FeedMirrorConfig {
            tick_every: Duration::from_millis(20),
            timeout: WAIT,
            ..FeedMirrorConfig::default()
        };
        let sync = FeedMirrorSync::start(&engine, store.clone(), mirror, config);
        Self {
            _dir: dir,
            clock,
            store,
            engine,
            sync,
            source,
            node,
        }
    }

    async fn until(&self, what: &str, done: impl Fn(&Self) -> bool) {
        let started = std::time::Instant::now();
        while !done(self) {
            assert!(started.elapsed() < WAIT, "{what}");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// Waits for the sync running (or starting now) to end.
    async fn settled(&self) {
        tokio::time::sleep(QUIET).await;
        self.until("the sync ends", |h| !h.sync.handle().status().syncing)
            .await;
    }
}

#[tokio::test]
async fn the_mirror_refreshes_at_start_after_a_failure_on_demand_and_never_while_off() {
    let h = Harness::start(Some(&[CORE])).await;
    h.until("the start's refresh", |h| {
        h.sync.handle().status().synced_at.is_some()
    })
    .await;
    let status = h.sync.handle().status();
    let [set] = status.sets.as_slice() else {
        panic!("{status:#?}");
    };
    assert_eq!(set.set.release, "24.10.4");
    assert_eq!(set.feeds[0].url, CORE);
    assert!(set.feeds[0].error.is_some(), "{set:#?}");
    assert!(status.sync_error.is_some());
    assert!(h.sync.handle().settings().enabled);
    let asked = h.source.asked();
    assert!(asked >= 1);
    assert!(h.source.0.lock().unwrap()[0].starts_with(CORE));

    // Not again before 30 minutes; then, as the sync failed.
    h.clock.advance(Duration::from_mins(29));
    h.settled().await;
    assert_eq!(h.source.asked(), asked, "too soon");
    h.clock.advance(Duration::from_mins(2));
    h.until("the retry", |h| h.source.asked() > asked).await;
    h.settled().await;

    // On demand.
    let asked = h.source.asked();
    h.sync.handle().refresh().await.unwrap();
    h.until("the refresh asked for", |h| h.source.asked() > asked)
        .await;
    h.settled().await;

    // A new feed is synced at once (with the others that have no index yet).
    let asked = h.source.asked();
    let mut inventory = h.store.inventory(h.node).await.unwrap().unwrap();
    inventory.feeds.push(NodeFeed {
        name: None,
        url: BASE.to_owned(),
    });
    h.store.set_inventory(h.node, inventory).await.unwrap();
    h.until("the new feed", |h| {
        h.source.0.lock().unwrap()[asked..]
            .iter()
            .any(|url| url.starts_with(BASE))
    })
    .await;
    h.settled().await;

    // Off: no sync, no refresh, nothing served.
    let off = MirrorSettings {
        enabled: false,
        whole_feeds: false,
    };
    h.store
        .put_setting(MirrorSettings::KEY, serde_json::to_value(off).unwrap())
        .await
        .unwrap();
    h.sync.handle().reload().await.unwrap();
    assert!(!h.sync.handle().settings().enabled);
    assert!(matches!(
        h.sync.handle().refresh().await,
        Err(FeedMirrorError::Off)
    ));
    let path = "downloads.openwrt.org/releases/24.10.4/targets/x86/64/packages/Packages.gz";
    assert!(matches!(
        h.sync.handle().serve(path).await,
        Err(ServeError::NotFound)
    ));
    let asked = h.source.asked();
    h.clock.advance(Duration::from_hours(13));
    h.settled().await;
    assert_eq!(h.source.asked(), asked, "nothing synced while off");
    h.sync.stop().await;
}

/// Adds the harness's node to the engine, its link answering the feeds' read.
async fn watch(h: &Harness) -> testing::Script {
    let (link, script) = testing::link();
    script.answer(|op, args| {
        (op == "exec" && args[0] == "sh").then(|| {
            let lines = "https://downloads.openwrt.org/releases/25.12.6/targets/x86/64/packages/packages.adb\n";
            Ok(json!({"code": 0, "stdout": lines}))
        })
    });
    let identity = Identity {
        role: NodeRole::Ap,
        mac: MacAddr::new([2, 0, 0, 0, 0, 2]),
    };
    h.engine.add(h.node, identity, link, None).await;
    script.set_state(running(hello(true)));
    script
}

#[tokio::test]
async fn a_node_whose_inventory_has_no_feeds_has_them_read_over_its_link() {
    let h = Harness::start(Some(&[])).await;
    let _script = watch(&h).await;
    let started = std::time::Instant::now();
    let feeds = loop {
        let feeds = h.store.inventory(h.node).await.unwrap().unwrap().feeds;
        if !feeds.is_empty() {
            break feeds;
        }
        assert!(started.elapsed() < WAIT, "the feeds are read");
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert_eq!(feeds.len(), 1);
    assert!(feeds[0].url.ends_with("/packages.adb"));
    h.until("the node's set is planned", |h| {
        h.sync.handle().status().sets.len() == 1
    })
    .await;
    h.sync.stop().await;
}

#[tokio::test]
async fn a_watched_node_without_an_inventory_is_planned_from_its_feeds_read_now() {
    let h = Harness::start(None).await;
    let _script = watch(&h).await;
    h.until("the node's set is planned", |h| {
        h.sync.handle().status().sets.len() == 1
    })
    .await;
    assert_eq!(
        h.store.inventory(h.node).await.unwrap(),
        None,
        "held in memory"
    );
    h.sync.stop().await;
}
