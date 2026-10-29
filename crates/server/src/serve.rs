//! `uniwrt serve`: starts the runtime, opens the database, binds the listener and serves until
//! SIGTERM or SIGINT.
//!
//! Shutdown is graceful: stop accepting, let in-flight requests finish for up to `DRAIN_TIMEOUT`
//! (inside Docker's default 10 s stop grace period), then close what is left. A second signal
//! closes everything at once. Then the engine stops and saves the nodes' last facts, and the
//! database closes last, after committing queued writes.

use std::{future::IntoFuture as _, io, path::Path, sync::Arc, time::Duration};

use anyhow::Context as _;
use axum::{Router, serve::ListenerExt as _};
use tokio::{
    net::TcpListener,
    signal::unix::{SignalKind, signal},
    sync::{Notify, mpsc},
};
use tracing::{info, warn};
use uniwrt_core::{Clock, SystemClock};
use uniwrt_node::ControllerKey;
use uniwrt_store::Store;

use crate::{
    build_info,
    cli::ServeArgs,
    controller_key,
    fleet::Fleet,
    http, logging,
    ui::{self, Source},
};

const DRAIN_TIMEOUT: Duration = Duration::from_secs(5);
/// The database file inside the data directory.
const DATABASE: &str = "uniwrt.db";

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
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let store = Store::open(data_dir.join(DATABASE), Arc::clone(&clock))
        .await
        .context("cannot open the database")?;
    let served = async {
        // Nodes trust this key; the engine connects with it.
        let controller_key = controller_key::load_or_generate(&store).await?;
        let listener = TcpListener::bind(args.http_listen)
            .await
            .with_context(|| format!("cannot listen on {}", args.http_listen))?;
        log_start(&listener, &data_dir, &controller_key)?;
        let fleet = Fleet::start(&store, &controller_key, clock).await?;
        let served = serve_until(listener, http::router(&ui::EMBEDDED), stop, DRAIN_TIMEOUT).await;
        fleet.stop().await;
        served?;
        anyhow::Ok(())
    }
    .await;
    store.close().await;
    served?;
    info!("uniwrt stopped");
    Ok(())
}

fn log_start(listener: &TcpListener, data_dir: &Path, key: &ControllerKey) -> io::Result<()> {
    info!(
        version = build_info::VERSION,
        commit = build_info::COMMIT,
        http = %listener.local_addr()?,
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

/// Serves `app` until the first stop request, then drains for up to `drain`; a second request,
/// or the end of the drain time, closes the remaining connections.
async fn serve_until(
    listener: TcpListener,
    app: Router,
    mut stop: mpsc::Receiver<&'static str>,
    drain: Duration,
) -> io::Result<()> {
    let draining = Arc::new(Notify::new());
    let listener = listener.tap_io(|tcp| {
        // Small API responses should not wait for Nagle's algorithm; failure only costs latency.
        let _ = tcp.set_nodelay(true);
    });
    let server = axum::serve(listener, app)
        .with_graceful_shutdown({
            let draining = Arc::clone(&draining);
            async move { draining.notified().await }
        })
        .into_future();
    tokio::pin!(server);
    tokio::select! {
        result = &mut server => return result,
        Some(reason) = stop.recv() => info!(reason, "shutting down: draining connections"),
    }
    draining.notify_one();
    tokio::select! {
        result = &mut server => result?,
        () = tokio::time::sleep(drain) => warn!(?drain, "connections still open after the drain time; closing them"),
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
        sync::{Notify, mpsc},
        task::JoinHandle,
        time::timeout,
    };

    use super::serve_until;
    use crate::{http, ui::EMBEDDED};

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
        (
            addr,
            tx,
            tokio::spawn(serve_until(listener, app, rx, drain)),
        )
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
        let (addr, stop, server) = start(http::router(&EMBEDDED), Duration::from_secs(5)).await;
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(b"GET /api/v1/health HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
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
