//! The per-command install path (D-147; features/lifecycle.md, Offline packages) with real
//! package managers: a mirror of an OpenWrt image's own feeds, synced from downloads.openwrt.org
//! and served over HTTPS on loopback the way the controller serves `/feeds`, and the image's opkg
//! or apk installing through it with the commands jobs run ([`Via::wrap`]): a package the sync
//! kept (tcpdump-mini, with libpcap) and one the mirror pulls through at the node's request
//! (iperf3); and, on 25.12's x86/64 image, the feeds of a node's next release derived from
//! 24.10.8's (M9.0c1b2). The container shares the host's network, so the login's address is
//! loopback. Run by scripts/test-dropbear.sh per image (`UNIWRT_IMAGE`); skips when the feeds
//! can't be reached.

use std::{collections::BTreeSet, net::SocketAddr, path::Path, process::Output, sync::Arc};

use axum::{
    Router,
    extract::{Path as UrlPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::{
    io::AsyncReadExt as _,
    net::{TcpListener, TcpStream},
    process::Command,
};
use tokio_rustls::{TlsAcceptor, server::TlsStream};
use uniwrt_core::SystemClock;
use uniwrt_feeds::{
    Feed, HttpSource, Mirror, Plan, ServeError, Served, SetPlan, Source as _, Validators,
};
use uniwrt_model::{
    facts::PackageManager,
    feeds::{FeedSet, NextRelease},
};
use uniwrt_node::pkg::{self, via::Via};

/// The container that runs the commands, removed when dropped.
struct Container(String);

impl Drop for Container {
    fn drop(&mut self) {
        let _ = std::process::Command::new("docker")
            .args(["rm", "-f", &self.0])
            .output();
    }
}

async fn docker<S: AsRef<std::ffi::OsStr>>(args: &[S]) -> Output {
    Command::new("docker")
        .args(args)
        .output()
        .await
        .expect("docker runs")
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

impl Container {
    /// Runs `argv` in it, as a login from `from` would.
    async fn exec(&self, from: &str, argv: &[String]) -> Output {
        let connection = format!("SSH_CONNECTION={from} 50022 {from} 22");
        let mut words = vec!["exec", "-e", &connection, &self.0];
        words.extend(argv.iter().map(String::as_str));
        docker(&words).await
    }

    async fn sh(&self, script: &str) -> String {
        let output = self
            .exec("", &["sh".to_owned(), "-c".to_owned(), script.to_owned()])
            .await;
        assert!(output.status.success(), "{script}: {}", text(&output));
        String::from_utf8_lossy(&output.stdout).into_owned()
    }
}

/// HTTPS on loopback with a certificate no node trusts, as the controller's.
struct Tls {
    tcp: TcpListener,
    acceptor: TlsAcceptor,
}

impl axum::serve::Listener for Tls {
    type Io = TlsStream<TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            let Ok((tcp, addr)) = self.tcp.accept().await else {
                continue;
            };
            if let Ok(tls) = self.acceptor.accept(tcp).await {
                return (tls, addr);
            }
        }
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        self.tcp.local_addr()
    }
}

async fn tls() -> Tls {
    let issued =
        rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_owned()]).expect("a certificate");
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(issued.signing_key.serialize_der()));
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .expect("TLS versions")
    .with_no_client_auth()
    .with_single_cert(vec![issued.cert.der().clone()], key)
    .expect("the certificate serves");
    Tls {
        tcp: TcpListener::bind("127.0.0.1:0").await.expect("a port"),
        acceptor: TlsAcceptor::from(Arc::new(config)),
    }
}

async fn serve(State(mirror): State<Mirror>, UrlPath(path): UrlPath<String>) -> Response {
    match mirror.serve(&path).await {
        Ok(Served { mut file, .. }) => {
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).await.expect("the file reads");
            bytes.into_response()
        }
        Err(ServeError::NotFound) => StatusCode::NOT_FOUND.into_response(),
        Err(ServeError::Unavailable(why)) => (StatusCode::BAD_GATEWAY, why).into_response(),
    }
}

/// Whether a file below `dir` is named `<name>…`.
fn holds(dir: &Path, name: &str) -> bool {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| {
            let path = entry.path();
            if path.is_dir() {
                holds(&path, name)
            } else {
                entry.file_name().to_string_lossy().starts_with(name)
            }
        })
}

/// The image's container (`UNIWRT_IMAGE`), named for `test`, on the host's network.
async fn start(test: &str) -> Container {
    let image = std::env::var("UNIWRT_IMAGE")
        .unwrap_or_else(|_| "openwrt/rootfs:x86-64-25.12.6".to_owned());
    let name = format!("uniwrt-{test}-{}", std::process::id());
    let started = docker(&[
        "run",
        "-d",
        "--rm",
        "--network",
        "host",
        "--name",
        &name,
        &image,
        "sleep",
        "900",
    ])
    .await;
    assert!(started.status.success(), "{}", text(&started));
    Container(name)
}

/// The node's package manager, release, target, architecture and feeds, as the controller
/// reads them.
async fn read_node(node: &Container) -> (PackageManager, [String; 3], Vec<Feed>) {
    let manager = if node.sh("command -v apk || true").await.trim().is_empty() {
        PackageManager::Opkg
    } else {
        PackageManager::Apk
    };
    let release = node
        .sh(". /etc/openwrt_release && echo \"$DISTRIB_RELEASE $DISTRIB_TARGET $DISTRIB_ARCH\"")
        .await;
    let [release, target, arch] = release.split_whitespace().collect::<Vec<_>>()[..] else {
        panic!("{release}");
    };
    let read = node
        .exec("", &pkg::feeds::command(manager).map(str::to_owned))
        .await;
    let feeds: Vec<Feed> = pkg::feeds::parse(manager, &String::from_utf8_lossy(&read.stdout))
        .iter()
        .filter_map(|feed| Feed::parse(&feed.url).ok())
        .collect();
    assert!(!feeds.is_empty(), "{}", text(&read));
    (manager, [release, target, arch].map(str::to_owned), feeds)
}

/// Serves `mirror` over HTTPS on loopback as the controller does; the nodes' way to it.
async fn serve_mirror(mirror: &Mirror) -> Via {
    let listener = tls().await;
    let port = listener.tcp.local_addr().expect("bound").port();
    let app = Router::new()
        .route("/feeds/{*path}", axum::routing::get(serve))
        .with_state(mirror.clone());
    tokio::spawn(async move { axum::serve(listener, app).await });
    let via = Via::new(port);
    via.set(true);
    via
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs Docker, an OpenWrt image (UNIWRT_IMAGE) and the internet"]
async fn an_image_installs_through_the_mirror_of_its_own_feeds() {
    let node = start("feeds").await;
    let (manager, [release, target, arch], feeds) = read_node(&node).await;
    let (release, target, arch) = (release.as_str(), target.as_str(), arch.as_str());

    // The mirror: the feeds' indexes and what is wanted of them.
    let dir = tempfile::tempdir().unwrap();
    let source = HttpSource::new("uniwrt-test").unwrap();
    let mirror = Mirror::open(dir.path().to_owned(), source, Arc::new(SystemClock))
        .await
        .unwrap();
    let plan = Plan {
        sets: vec![SetPlan {
            set: FeedSet {
                release: release.to_owned(),
                target: target.to_owned(),
                arch: arch.to_owned(),
                manager,
            },
            feeds,
            wanted: BTreeSet::from(["tcpdump-mini".to_owned()]),
            offered: false,
        }],
        whole: false,
        images: Vec::new(),
    };
    let synced = mirror.sync(plan, true).await;
    let status = mirror.status();
    if status.sets[0]
        .feeds
        .iter()
        .all(|feed| feed.index_at.is_none())
    {
        eprintln!("skipped: the feeds can't be reached ({synced:?}: {status:#?})");
        return;
    }
    assert!(holds(dir.path(), "tcpdump-mini"), "{status:#?}");
    assert!(!holds(dir.path(), "iperf3"));

    let via = serve_mirror(&mirror).await;
    node.sh("mkdir -p /var/lock /var/opkg-lists").await;
    for argv in [
        pkg::update(manager),
        pkg::install(manager, &["tcpdump-mini", "iperf3"]),
    ] {
        let output = node.exec("127.0.0.1", &via.wrap(argv.clone())).await;
        assert!(output.status.success(), "{argv:?}: {}", text(&output));
    }
    node.sh("command -v tcpdump && command -v iperf3").await;
    // iperf3 came through the mirror: it holds it now, kept for good.
    assert!(holds(dir.path(), "iperf3"), "{:#?}", mirror.status());

    // Without the controller, the same command runs against the node's own feeds.
    let unreachable = Via::new(1);
    unreachable.set(true);
    let wrapped = unreachable.wrap(vec!["echo".to_owned(), "as it is".to_owned()]);
    let output = node.exec("127.0.0.1", &wrapped).await;
    assert_eq!(String::from_utf8_lossy(&output.stdout), "as it is\n");
}

/// 24.10.8's x86/64 feeds (its `/etc/opkg/distfeeds.conf`): the release before 25.12.
const LAST_BRANCH: [&str; 7] = [
    "https://downloads.openwrt.org/releases/24.10.8/targets/x86/64/packages",
    "https://downloads.openwrt.org/releases/24.10.8/packages/x86_64/base",
    "https://downloads.openwrt.org/releases/24.10.8/targets/x86/64/kmods/6.6.144-1-50daf8372d971124fb3519e8d87e02ae",
    "https://downloads.openwrt.org/releases/24.10.8/packages/x86_64/luci",
    "https://downloads.openwrt.org/releases/24.10.8/packages/x86_64/packages",
    "https://downloads.openwrt.org/releases/24.10.8/packages/x86_64/routing",
    "https://downloads.openwrt.org/releases/24.10.8/packages/x86_64/telephony",
];

/// M9.0c1b2: the mirror keeps a node's next release ahead, its feeds derived from the ones it
/// runs and the kernel modules' directory from the release's `profiles.json`. On a 25.12
/// x86/64 image, as a 24.10.8 node upgraded to it: the derived feeds are the image's own (but
/// 25.12's new `video`), and its apk installs through a mirror of them.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs Docker, an OpenWrt image (UNIWRT_IMAGE) and the internet"]
async fn an_upgraded_node_installs_through_the_feeds_derived_from_its_last_release() {
    let node = start("next").await;
    let (manager, [release, target, arch], own) = read_node(&node).await;
    if !release.starts_with("25.12.") || target != "x86/64" {
        eprintln!("skipped: {release} {target} isn't the x86/64 release after 24.10.8");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let source = HttpSource::new("uniwrt-test").unwrap();
    let profiles = dir.path().join("profiles.json");
    let url =
        format!("https://downloads.openwrt.org/releases/{release}/targets/{target}/profiles.json");
    if let Err(err) = source
        .fetch(&url, &Validators::default(), &profiles, 8 << 20)
        .await
    {
        eprintln!("skipped: {url} can't be had ({err})");
        return;
    }
    let profiles: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&profiles).unwrap()).unwrap();
    let kernel = &profiles["linux_kernel"];
    let part = |key: &str| kernel[key].as_str().unwrap().to_owned();
    let next = NextRelease {
        release: release.clone(),
        target: target.clone(),
        arch: arch.clone(),
        manager: Some(manager),
        kmods: Some(format!(
            "{}-{}-{}",
            part("version"),
            part("release"),
            part("vermagic")
        )),
        packages: Vec::new(),
        images: Vec::new(),
    };
    let last: Vec<Feed> = LAST_BRANCH
        .iter()
        .map(|url| Feed::parse(url).unwrap())
        .collect();
    let derived = uniwrt_feeds::next::derive(&last, "24.10.8", "x86_64", &next);
    let missing: Vec<String> = own
        .iter()
        .filter(|feed| !derived.contains(feed))
        .map(Feed::url)
        .collect();
    assert!(
        derived.iter().all(|feed| own.contains(feed)) && missing.len() == 1,
        "derived {derived:#?}, the image's own but these: {missing:?}"
    );
    assert!(missing[0].ends_with("/video"), "{missing:?}");

    let mirror = Mirror::open(dir.path().join("feeds"), source, Arc::new(SystemClock))
        .await
        .unwrap();
    let plan = Plan {
        sets: vec![SetPlan {
            set: FeedSet {
                release,
                target,
                arch,
                manager,
            },
            feeds: derived,
            wanted: BTreeSet::from(["tcpdump-mini".to_owned()]),
            offered: true,
        }],
        whole: false,
        images: Vec::new(),
    };
    let synced = mirror.sync(plan, true).await;
    let status = mirror.status();
    if status.sets[0]
        .feeds
        .iter()
        .all(|feed| feed.index_at.is_none())
    {
        eprintln!("skipped: the feeds can't be reached ({synced:?}: {status:#?})");
        return;
    }
    assert!(
        holds(&dir.path().join("feeds"), "tcpdump-mini"),
        "{status:#?}"
    );
    let via = serve_mirror(&mirror).await;
    for argv in [
        pkg::update(manager),
        pkg::install(manager, &["tcpdump-mini"]),
    ] {
        let output = node.exec("127.0.0.1", &via.wrap(argv.clone())).await;
        assert!(output.status.success(), "{argv:?}: {}", text(&output));
    }
    node.sh("command -v tcpdump").await;
}
