//! `uniwrt serve`: starts the runtime, opens the database and the vault, binds the HTTP and
//! HTTPS listeners and serves until SIGTERM or SIGINT.
//!
//! Shutdown is graceful: stop accepting, let in-flight requests finish for up to `DRAIN_TIMEOUT`
//! (inside Docker's default 10 s stop grace period), then close what is left. A second signal
//! closes everything at once. Then the engine stops and saves the nodes' last facts, and the
//! database closes last, after committing queued writes.
//!
//! With `--demo` the nodes are simulated, and the database lives in a `demo-*` directory
//! inside the data directory (a scratch image has no /tmp), deleted at exit; its master key
//! exists only in memory.

use std::{
    future::IntoFuture as _, io, net::SocketAddr, path::Path, pin::Pin, sync::Arc, time::Duration,
};

use anyhow::Context as _;
use axum::Router;
use tokio::{
    net::TcpListener,
    signal::unix::{SignalKind, signal},
    sync::{mpsc, watch},
    task::JoinSet,
};
use tracing::{info, warn};
use uniwrt_api::{Api, ApiConfig, Certificate as _};
use uniwrt_core::{Clock, SystemClock};
use uniwrt_node::ControllerKey;
use uniwrt_sim::SimConfig;
use uniwrt_store::{Store, Vault};

use crate::{
    build_info,
    cli::ServeArgs,
    controller_key,
    fleet::Fleet,
    http, logging,
    master_key::{self, Source as KeySource},
    tls::{
        self,
        listener::{Peer, PlainListener, TlsListener},
    },
    ui::{self, Source},
};

const DRAIN_TIMEOUT: Duration = Duration::from_secs(5);
/// The database file inside the data directory.
const DATABASE: &str = "uniwrt.db";
/// The master key file inside the data directory, unless configured elsewhere.
const MASTER_KEY: &str = "master.key";
/// Demo mode's admin: its database is thrown away at exit, and its nodes are simulated.
const DEMO_USER: &str = "demo";
const DEMO_PASSWORD: &str = "demo";

/// One listener's server, running until its drain signal and its connections end.
type Server = Pin<Box<dyn Future<Output = io::Result<()>> + Send>>;

pub(crate) fn run(args: ServeArgs) -> anyhow::Result<()> {
    logging::init(&args.log)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("cannot start the async runtime")?;
    let result = runtime.block_on(serve(args));
    runtime.shutdown_timeout(Duration::from_secs(1));
    result
}

async fn serve(args: ServeArgs) -> anyhow::Result<()> {
    let data_dir = std::path::absolute(&args.data_dir)?;
    tokio::fs::create_dir_all(&data_dir)
        .await
        .with_context(|| format!("cannot create the data directory {}", data_dir.display()))?;
    // Registered before binding, so an early signal is not lost.
    let stop = forward_signals().context("cannot install signal handlers")?;
    let demo_dir = args
        .demo
        .then(|| {
            tempfile::Builder::new()
                .prefix("demo-")
                .tempdir_in(&data_dir)
        })
        .transpose()
        .context("cannot create the demo's directory")?;
    let state_dir = demo_dir
        .as_ref()
        .map_or_else(|| data_dir.clone(), |dir| dir.path().to_owned());
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let store = Store::open(state_dir.join(DATABASE), Arc::clone(&clock))
        .await
        .context("cannot open the database")?;
    let served = async {
        let Secured {
            vault,
            controller_key,
            tls,
            renewer,
        } = secure(&args, &data_dir, &store, &clock).await?;
        let http = bind(args.http_listen).await?;
        let https = bind(args.https_listen).await?;
        log_start(&http, &https, &state_dir, &controller_key)?;
        info!(names = ?tls.info().names, ca_sha256 = %tls.info().ca_sha256, "HTTPS certificate");
        let fleet = start_fleet(&args, &store, &controller_key, &clock).await?;
        let jobs = fleet
            .jobs(&store, vault, &controller_key, Arc::clone(&clock))
            .await?;
        let api = Api::new(ApiConfig::new(
            store.clone(),
            Arc::new(fleet.engine.clone()),
            Arc::clone(&clock),
            build_info::API,
            Arc::new(tls.clone()),
            jobs.clone(),
        ))
        .await
        .context("cannot start the API")?;
        if args.demo {
            api.create_admin(DEMO_USER, DEMO_PASSWORD).await?;
            info!(
                "demo mode: log in as {DEMO_USER}, password {DEMO_PASSWORD}; the last access point \
                 waits to be adopted, with root password {}",
                crate::demo::ROOT_PASSWORD
            );
        } else if api.awaiting_setup() {
            warn!("no users yet: create the admin in the web UI within 10 minutes (a restart reopens setup)");
        }
        let renewal = tokio::spawn(renewer.run());
        let app = http::router(&ui::EMBEDDED, api.clone());
        let plain = http::plain::router(
            app.clone(),
            args.https_listen.port(),
            args.trusted_proxies.clone(),
        );
        let (drain, draining) = watch::channel(false);
        // Draining waits for HTTP requests only; WebSockets are told to go.
        let mut closing = draining.clone();
        tokio::spawn(async move {
            if closing.wait_for(|draining| *draining).await.is_ok() {
                api.close_sockets();
            }
        });
        let servers = vec![
            https_server(
                TlsListener::new(https, tls.server_config()?)?,
                http::https(app),
                draining.clone(),
            ),
            http_server(http, plain, draining),
        ];
        let served = serve_until(servers, &drain, stop, DRAIN_TIMEOUT).await;
        renewal.abort();
        jobs.shutdown().await;
        fleet.stop().await;
        served?;
        anyhow::Ok(())
    }
    .await;
    store.close().await;
    if let Some(dir) = demo_dir
        && let Err(err) = dir.close()
    {
        warn!(%err, "cannot delete the demo's directory");
    }
    served?;
    info!("uniwrt stopped");
    Ok(())
}

/// The fleet: the simulated one in demo mode, else the adopted nodes.
async fn start_fleet(
    args: &ServeArgs,
    store: &Store,
    controller_key: &ControllerKey,
    clock: &Arc<dyn Clock>,
) -> anyhow::Result<Fleet> {
    if args.demo {
        let config = SimConfig {
            seed: args.demo_seed,
            ..SimConfig::default()
        };
        info!(
            seed = config.seed,
            "demo mode: simulating {} access points and {} clients; its data is deleted at exit",
            config.access_points,
            config.clients
        );
        Fleet::demo(store, &config, Arc::clone(clock)).await
    } else {
        Fleet::start(store, controller_key, Arc::clone(clock)).await
    }
}

/// The vault, and what it holds and serves: the controller's SSH key and HTTPS.
struct Secured {
    vault: Vault,
    controller_key: ControllerKey,
    tls: tls::Tls,
    renewer: tls::Renewer,
}

async fn secure(
    args: &ServeArgs,
    data_dir: &Path,
    store: &Store,
    clock: &Arc<dyn Clock>,
) -> anyhow::Result<Secured> {
    let key_file = args
        .master_key_file
        .clone()
        .unwrap_or_else(|| data_dir.join(MASTER_KEY));
    let key_source = if args.demo {
        KeySource::Ephemeral
    } else {
        KeySource::Configured(&key_file)
    };
    let vault = master_key::open_vault(store, key_source).await?;
    // Nodes trust this key; the engine connects with it.
    let controller_key = controller_key::load_or_generate(&vault).await?;
    let (tls, renewer) = tls::start(
        vault.clone(),
        Arc::clone(clock),
        args.tls_names.clone(),
        vec![args.https_listen],
    )
    .await
    .context("cannot set up HTTPS")?;
    Ok(Secured {
        vault,
        controller_key,
        tls,
        renewer,
    })
}

async fn bind(addr: SocketAddr) -> anyhow::Result<TcpListener> {
    TcpListener::bind(addr)
        .await
        .with_context(|| format!("cannot listen on {addr}"))
}

fn log_start(
    http: &TcpListener,
    https: &TcpListener,
    data_dir: &Path,
    key: &ControllerKey,
) -> io::Result<()> {
    info!(
        version = build_info::VERSION,
        commit = build_info::COMMIT,
        https = %https.local_addr()?,
        http = %http.local_addr()?,
        data_dir = %data_dir.display(),
        controller_key = %key.fingerprint(),
        "uniwrt is serving"
    );
    if ui::EMBEDDED.source == Source::Placeholder {
        warn!("this build has no web UI; serving a placeholder page");
    }
    Ok(())
}

/// Forwards SIGTERM and SIGINT, by name, for as long as the receiver lives.
fn forward_signals() -> io::Result<mpsc::Receiver<&'static str>> {
    let mut terminate = signal(SignalKind::terminate())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    let (tx, rx) = mpsc::channel(2);
    tokio::spawn(async move {
        loop {
            let name = tokio::select! {
                _ = terminate.recv() => "SIGTERM",
                _ = interrupt.recv() => "SIGINT",
            };
            if tx.send(name).await.is_err() {
                break;
            }
        }
    });
    Ok(rx)
}

/// Resolves once `draining` turns true (or its sender is gone).
async fn drained(mut draining: watch::Receiver<bool>) {
    let _ = draining.wait_for(|draining| *draining).await;
}

fn https_server(listener: TlsListener, app: Router, draining: watch::Receiver<bool>) -> Server {
    let service = app.into_make_service_with_connect_info::<Peer>();
    Box::pin(
        axum::serve(listener, service)
            .with_graceful_shutdown(drained(draining))
            .into_future(),
    )
}

fn http_server(listener: TcpListener, app: Router, draining: watch::Receiver<bool>) -> Server {
    let listener = PlainListener(listener);
    let service = app.into_make_service_with_connect_info::<Peer>();
    Box::pin(
        axum::serve(listener, service)
            .with_graceful_shutdown(drained(draining))
            .into_future(),
    )
}

/// Runs `servers` until the first stop request, then tells them to drain (through `drain`, which
/// their graceful shutdown waits on) for up to `drain_time`; a second request, or the end of the
/// drain time, closes the remaining connections. A server that fails ends them all.
async fn serve_until(
    servers: Vec<Server>,
    drain: &watch::Sender<bool>,
    mut stop: mpsc::Receiver<&'static str>,
    drain_time: Duration,
) -> io::Result<()> {
    let mut running = JoinSet::new();
    for server in servers {
        running.spawn(server);
    }
    let ended = |joined: Result<io::Result<()>, tokio::task::JoinError>| {
        joined.unwrap_or_else(|err| Err(io::Error::other(err)))
    };
    tokio::select! {
        Some(joined) = running.join_next() => return ended(joined),
        Some(reason) = stop.recv() => info!(reason, "shutting down: draining connections"),
    }
    drain.send_replace(true);
    let all = async {
        while let Some(joined) = running.join_next().await {
            ended(joined)?;
        }
        io::Result::Ok(())
    };
    tokio::select! {
        result = all => result?,
        () = tokio::time::sleep(drain_time) => warn!(?drain_time, "connections still open after the drain time; closing them"),
        Some(reason) = stop.recv() => warn!(reason, "second stop request; closing connections now"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        net::SocketAddr,
        sync::Arc,
        time::{Duration, Instant},
    };

    use axum::{Router, routing::get};
    use tokio::{
        io::{AsyncReadExt as _, AsyncWriteExt as _},
        net::{TcpListener, TcpStream},
        sync::{Notify, mpsc, watch},
        task::JoinHandle,
        time::timeout,
    };

    use super::{http_server, https_server, serve_until};
    use crate::{
        http::{self, test_api},
        tls::{Tls, listener::TlsListener},
        ui::EMBEDDED,
    };

    async fn start(
        app: Router,
        drain: Duration,
    ) -> (
        SocketAddr,
        mpsc::Sender<&'static str>,
        JoinHandle<std::io::Result<()>>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel(2);
        let (drain_tx, draining) = watch::channel(false);
        let servers = vec![http_server(listener, app, draining)];
        let server = tokio::spawn(async move { serve_until(servers, &drain_tx, rx, drain).await });
        (addr, tx, server)
    }

    async fn fetch(addr: SocketAddr, path: &str) -> String {
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let request =
            format!("GET {path} HTTP/1.1\r\nHost: uniwrt.lan:8080\r\nConnection: close\r\n\r\n");
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        response
    }

    /// Starts a server whose only route never answers, with one request stuck inside it.
    async fn start_stuck(
        drain: Duration,
    ) -> (
        TcpStream,
        mpsc::Sender<&'static str>,
        JoinHandle<std::io::Result<()>>,
    ) {
        let entered = Arc::new(Notify::new());
        let handler = {
            let entered = Arc::clone(&entered);
            move || {
                entered.notify_one();
                std::future::pending::<&'static str>()
            }
        };
        let (addr, stop, server) = start(Router::new().route("/stuck", get(handler)), drain).await;
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(b"GET /stuck HTTP/1.1\r\nHost: test\r\n\r\n")
            .await
            .unwrap();
        entered.notified().await;
        (stream, stop, server)
    }

    #[tokio::test]
    async fn serves_then_stops_on_request() {
        let (_dir, api) = test_api().await;
        let (addr, stop, server) =
            start(http::router(&EMBEDDED, api), Duration::from_secs(5)).await;
        let response = fetch(addr, "/api/v1/health").await;
        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"), "{response}");

        stop.send("test").await.unwrap();
        timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn https_and_the_redirecting_http_listener_stop_together() {
        let tls = Tls::for_tests();
        let https = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let http = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let http_addr = http.local_addr().unwrap();
        let (_dir, api) = test_api().await;
        let app = http::router(&EMBEDDED, api);
        let plain = http::plain::router(app.clone(), 8443, Vec::new());
        let (drain_tx, draining) = watch::channel(false);
        let servers = vec![
            https_server(
                TlsListener::new(https, tls.server_config().unwrap()).unwrap(),
                http::https(app),
                draining.clone(),
            ),
            http_server(http, plain, draining),
        ];
        let (stop, rx) = mpsc::channel(2);
        let server = tokio::spawn(async move {
            serve_until(servers, &drain_tx, rx, Duration::from_secs(5)).await
        });

        let response = fetch(http_addr, "/nodes").await;
        assert!(
            response.starts_with("HTTP/1.1 307 Temporary Redirect\r\n"),
            "{response}"
        );
        assert!(
            response.contains("location: https://uniwrt.lan:8443/nodes\r\n"),
            "{response}"
        );
        let response = fetch(http_addr, "/api/v1/health").await;
        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"), "{response}");

        stop.send("test").await.unwrap();
        timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn stuck_requests_are_cut_off_after_the_drain_time() {
        let (_stream, stop, server) = start_stuck(Duration::from_millis(100)).await;
        let started = Instant::now();
        stop.send("test").await.unwrap();
        timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(started.elapsed() >= Duration::from_millis(100));
    }

    #[tokio::test]
    async fn a_second_stop_request_closes_at_once() {
        let (_stream, stop, server) = start_stuck(Duration::from_secs(60)).await;
        stop.send("first").await.unwrap();
        stop.send("second").await.unwrap();
        timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
}
