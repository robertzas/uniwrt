//! The per-command install path (D-147; features/lifecycle.md, Offline packages) with real
//! package managers: a mirror of an OpenWrt image's own feeds, synced from downloads.openwrt.org
//! and served over HTTPS on loopback the way the controller serves `/feeds`, and the image's opkg
//! or apk installing through it with the commands jobs run ([`Via::wrap`]): a package the sync
//! kept (tcpdump-mini, with libpcap) and one the mirror pulls through at the node's request
//! (iperf3). The container shares the host's network, so the login's address is loopback. Run
//! by scripts/test-dropbear.sh per image (`UNIWRT_IMAGE`); skips when the feeds can't be reached.

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
use uniwrt_feeds::{Feed, HttpSource, Mirror, Plan, ServeError, Served, SetPlan};
use uniwrt_model::{facts::PackageManager, feeds::FeedSet};
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

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs Docker, an OpenWrt image (UNIWRT_IMAGE) and the internet"]
async fn an_image_installs_through_the_mirror_of_its_own_feeds() {
    let image = std::env::var("UNIWRT_IMAGE")
        .unwrap_or_else(|_| "openwrt/rootfs:x86-64-25.12.6".to_owned());
    let name = format!("uniwrt-feeds-{}", std::process::id());
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
    let node = Container(name);

    // The node's release and feeds, as the controller reads them.
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
        }],
        whole: false,
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

    let listener = tls().await;
    let port = listener.tcp.local_addr().unwrap().port();
    let app = Router::new()
        .route("/feeds/{*path}", axum::routing::get(serve))
        .with_state(mirror.clone());
    tokio::spawn(async move { axum::serve(listener, app).await });

    let via = Via::new(port);
    via.set(true);
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
