//! A throughput test's far end (D-142), set up before the client starts on the run's node: on
//! another node, `iperf3 -s -1` started and waited for until it listens; on the controller, its
//! own server ([`uniwrt_node::iperf3`]) bound for the test and taking only the node's
//! addresses. The far end lives as long as the [`FarEnd`] guard; its errors join the run's
//! output.

use std::{
    net::{IpAddr, Ipv6Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use tokio::{net::TcpListener, task::JoinHandle, time};
use uniwrt_core::NodeId;
use uniwrt_model::diag::iperf3::{Iperf3Direction, MAX_SECONDS, MAX_STREAMS};
use uniwrt_node::{
    ExitStatus, Output,
    diag::iperf3::{self, Far},
    iperf3::ServerConfig,
};

use super::{Inner, run::Splitter};

/// How long the far node's server may take to listen (SSH setup included).
const LISTEN_TIMEOUT: Duration = Duration::from_secs(15);
/// How long each step of the controller's handshake may take.
const HANDSHAKE: Duration = Duration::from_secs(10);
/// The far node's words kept while it starts.
const MAX_SAID: usize = 8;

/// A test as the run plans it.
#[derive(Debug, Clone)]
pub(super) struct Test {
    pub(super) direction: Iperf3Direction,
    pub(super) seconds: u32,
    pub(super) streams: u32,
    pub(super) far: Peer,
}

/// Where the far end runs.
#[derive(Debug, Clone)]
pub(super) enum Peer {
    /// Another node, managed at `host`.
    Node { node: NodeId, host: String },
    /// The controller, listening at `listen` for the run's node, managed at `host`.
    Controller { listen: SocketAddr, host: String },
}

/// Keeps the far end running; dropping it ends it.
pub(super) struct FarEnd(JoinHandle<()>);

impl Drop for FarEnd {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Starts the far end; answers the client's command and the guard, or why it can't start.
pub(super) async fn prepare(
    inner: &Arc<Inner>,
    id: u32,
    test: &Test,
) -> Result<(Vec<String>, FarEnd), String> {
    match &test.far {
        Peer::Node { node, host } => {
            let far = far_node(inner, id, *node).await?;
            let argv = iperf3::client(
                test.direction,
                test.seconds,
                test.streams,
                Far::Node { host },
            );
            Ok((argv, far))
        }
        Peer::Controller { listen, host } => {
            let allowed = addresses(host).await?;
            let listener = bind(*listen).await?;
            let port = listener
                .local_addr()
                .map_err(|err| format!("The controller's iperf3 server has no port: {err}"))?
                .port();
            let config = ServerConfig {
                allowed,
                max_seconds: MAX_SECONDS,
                max_streams: MAX_STREAMS,
                handshake: HANDSHAKE,
            };
            let inner = Arc::clone(inner);
            let task = tokio::spawn(async move {
                if let Err(err) = uniwrt_node::iperf3::serve(listener, config).await {
                    inner.add(id, vec![format!("controller: {err}")], true);
                }
            });
            let argv = iperf3::client(
                test.direction,
                test.seconds,
                test.streams,
                Far::Controller { port },
            );
            Ok((argv, FarEnd(task)))
        }
    }
}

/// Starts `iperf3 -s -1` on `node` and waits until it listens; then passes its errors on to the
/// run.
async fn far_node(inner: &Arc<Inner>, id: u32, node: NodeId) -> Result<FarEnd, String> {
    let max = inner.config.max_line;
    let wait = async {
        let mut stream = (inner.start)(node, iperf3::server())
            .await
            .map_err(|err| super::run::worded(&err, "Cannot start iperf3 on the far device"))?;
        let (mut out, mut err) = (Splitter::new(max), Splitter::new(max));
        let mut said: Vec<String> = Vec::new();
        loop {
            match stream.next().await {
                Some(Ok(Output::Stdout(data))) => {
                    let lines = out.push(&data);
                    if lines.iter().any(|line| line.contains(iperf3::LISTENING)) {
                        return Ok((stream, err));
                    }
                    keep(&mut said, lines);
                }
                Some(Ok(Output::Stderr(data))) => keep(&mut said, err.push(&data)),
                Some(Ok(Output::Exit(ExitStatus::Code(127)))) => {
                    return Err(
                        "The far device lacks iperf3: install the iperf3 package".to_owned()
                    );
                }
                Some(Ok(Output::Exit(_))) | None => {
                    keep(
                        &mut said,
                        out.finish().into_iter().chain(err.finish()).collect(),
                    );
                    return Err(if said.is_empty() {
                        "iperf3 ended on the far device before it listened".to_owned()
                    } else {
                        format!("iperf3 didn't start on the far device: {}", said.join(" "))
                    });
                }
                Some(Err(error)) => {
                    return Err(super::run::worded(
                        &error,
                        "iperf3 failed on the far device",
                    ));
                }
            }
        }
    };
    let (mut stream, mut err) = time::timeout(LISTEN_TIMEOUT, wait).await.map_err(|_| {
        format!(
            "iperf3 didn't listen on the far device within {} s",
            LISTEN_TIMEOUT.as_secs()
        )
    })??;
    let inner = Arc::clone(inner);
    let task = tokio::spawn(async move {
        let far = |lines: Vec<String>| -> Vec<String> {
            lines
                .into_iter()
                .map(|line| format!("far end: {line}"))
                .collect()
        };
        while let Some(Ok(output)) = stream.next().await {
            if let Output::Stderr(data) = output {
                inner.add(id, far(err.push(&data)), true);
            }
        }
        inner.add(id, far(err.finish().into_iter().collect()), true);
    });
    Ok(FarEnd(task))
}

/// Keeps the far node's words while it starts, without iperf3's rules (`-----`).
fn keep(said: &mut Vec<String>, lines: Vec<String>) {
    said.extend(
        lines
            .into_iter()
            .filter(|line| !line.trim_matches('-').trim().is_empty()),
    );
    said.truncate(MAX_SAID);
}

/// The addresses `host` (an address, or a name) stands for.
async fn addresses(host: &str) -> Result<Vec<IpAddr>, String> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(vec![ip]);
    }
    let found = tokio::net::lookup_host((host, 0))
        .await
        .map_err(|err| format!("Cannot resolve the device's address {host}: {err}"))?;
    let mut ips: Vec<IpAddr> = found.map(|addr| addr.ip()).collect();
    ips.dedup();
    if ips.is_empty() {
        return Err(format!("The device's address {host} resolves to nothing"));
    }
    Ok(ips)
}

/// Binds the controller's server; an unspecified IPv6 address falls back to IPv4's where the
/// host has no IPv6.
async fn bind(listen: SocketAddr) -> Result<TcpListener, String> {
    let refused =
        |err: std::io::Error| format!("The controller cannot listen for iperf3 on {listen}: {err}");
    match TcpListener::bind(listen).await {
        Ok(listener) => Ok(listener),
        Err(err) if listen.ip() == IpAddr::V6(Ipv6Addr::UNSPECIFIED) => {
            TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, listen.port()))
                .await
                .map_err(|_| refused(err))
        }
        Err(err) => Err(refused(err)),
    }
}
