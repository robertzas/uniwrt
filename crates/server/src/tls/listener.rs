//! [`TlsListener`]: TLS on top of a TCP listener, as an axum [`Listener`]; [`PlainListener`], the
//! HTTP listener's; and [`Peer`], the client's address that handlers on either listener extract
//! with `ConnectInfo<Peer>`.
//!
//! Handshakes run in tasks of their own, at most [`MAX_HANDSHAKES`] at once and each for at most
//! [`HANDSHAKE_TIMEOUT`], so a slow or silent client never holds up the others.

use std::{io, net::SocketAddr, sync::Arc, time::Duration};

use axum::{
    extract::connect_info::Connected,
    serve::{IncomingStream, Listener},
};
use rustls::ServerConfig;
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{Semaphore, mpsc},
    task::JoinHandle,
    time::timeout,
};
use tokio_rustls::{TlsAcceptor, server::TlsStream};
use tracing::debug;

pub(crate) const MAX_HANDSHAKES: usize = 128;
pub(crate) const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// The client's address, on the HTTP and the HTTPS listener alike.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Peer(pub(crate) SocketAddr);

impl Connected<IncomingStream<'_, TlsListener>> for Peer {
    fn connect_info(stream: IncomingStream<'_, TlsListener>) -> Self {
        Self(*stream.remote_addr())
    }
}

impl Connected<IncomingStream<'_, PlainListener>> for Peer {
    fn connect_info(stream: IncomingStream<'_, PlainListener>) -> Self {
        Self(*stream.remote_addr())
    }
}

/// Plain TCP, with `TCP_NODELAY` on every connection.
pub(crate) struct PlainListener(pub(crate) TcpListener);

impl Listener for PlainListener {
    type Io = TcpStream;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        let (stream, peer) = accept_tcp(&self.0).await;
        (stream, peer)
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        self.0.local_addr()
    }
}

/// The next connection, with `TCP_NODELAY` set: small API responses should not wait for
/// Nagle's algorithm (failing to set it only costs latency). Errors are retried as axum's own
/// listener does: they concern one connection, or a full descriptor table that a pause may
/// relieve.
async fn accept_tcp(tcp: &TcpListener) -> (TcpStream, SocketAddr) {
    loop {
        match tcp.accept().await {
            Ok((stream, peer)) => {
                let _ = stream.set_nodelay(true);
                return (stream, peer);
            }
            Err(err) => {
                debug!(%err, "accept failed");
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

/// Accepts TCP connections and hands out those whose TLS handshake completed.
pub(crate) struct TlsListener {
    handshaken: mpsc::Receiver<(TlsStream<TcpStream>, SocketAddr)>,
    local_addr: SocketAddr,
    acceptor: JoinHandle<()>,
}

impl TlsListener {
    pub(crate) fn new(tcp: TcpListener, config: Arc<ServerConfig>) -> io::Result<Self> {
        let local_addr = tcp.local_addr()?;
        let (tx, handshaken) = mpsc::channel(MAX_HANDSHAKES);
        let acceptor = tokio::spawn(accept(tcp, TlsAcceptor::from(config), tx));
        Ok(Self {
            handshaken,
            local_addr,
            acceptor,
        })
    }
}

impl Drop for TlsListener {
    fn drop(&mut self) {
        self.acceptor.abort();
    }
}

async fn accept(
    tcp: TcpListener,
    acceptor: TlsAcceptor,
    tx: mpsc::Sender<(TlsStream<TcpStream>, SocketAddr)>,
) {
    let handshakes = Arc::new(Semaphore::new(MAX_HANDSHAKES));
    loop {
        let (stream, peer) = accept_tcp(&tcp).await;
        let Ok(permit) = Arc::clone(&handshakes).try_acquire_owned() else {
            debug!(%peer, "too many TLS handshakes in progress; dropping the connection");
            continue;
        };
        let (acceptor, tx) = (acceptor.clone(), tx.clone());
        tokio::spawn(async move {
            match timeout(HANDSHAKE_TIMEOUT, acceptor.accept(stream)).await {
                Ok(Ok(tls)) => {
                    drop(permit);
                    let _ = tx.send((tls, peer)).await;
                }
                // Browsers that do not trust the certificate yet end handshakes this way.
                Ok(Err(err)) => debug!(%peer, %err, "TLS handshake failed"),
                Err(_) => debug!(%peer, "TLS handshake timed out"),
            }
        });
    }
}

impl Listener for TlsListener {
    type Io = TlsStream<TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        match self.handshaken.recv().await {
            Some(accepted) => accepted,
            // The acceptor only ends when aborted, which happens only when this is dropped.
            None => std::future::pending().await,
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        Ok(self.local_addr)
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use axum::{Router, extract::ConnectInfo, routing::get};
    use rustls::{
        ClientConfig, RootCertStore,
        pki_types::{CertificateDer, ServerName, pem::PemObject as _},
    };
    use tokio::{
        io::{AsyncReadExt as _, AsyncWriteExt as _},
        net::{TcpListener, TcpStream},
    };
    use tokio_rustls::TlsConnector;

    use uniwrt_api::Certificate as _;

    use super::{Peer, TlsListener};
    use crate::tls::Tls;

    fn client(tls: &Tls) -> TlsConnector {
        let mut roots = RootCertStore::empty();
        roots
            .add(CertificateDer::from_pem_slice(tls.ca_pem().as_bytes()).unwrap())
            .unwrap();
        let config =
            ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_root_certificates(roots)
                .with_no_client_auth();
        TlsConnector::from(Arc::new(config))
    }

    #[tokio::test]
    async fn serves_https_to_a_client_that_trusts_the_ca_while_a_silent_one_waits() {
        let tls = Tls::for_tests();
        let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = tcp.local_addr().unwrap();
        let listener = TlsListener::new(tcp, tls.server_config().unwrap()).unwrap();
        let app = Router::new().route(
            "/",
            get(|ConnectInfo(Peer(peer)): ConnectInfo<Peer>| async move { peer.ip().to_string() }),
        );
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service_with_connect_info::<Peer>())
                .await
                .unwrap();
        });

        // A client that connects and never says hello does not hold up the next one.
        let _silent = TcpStream::connect(addr).await.unwrap();

        let tcp = TcpStream::connect(addr).await.unwrap();
        let name = ServerName::try_from("uniwrt.lan").unwrap();
        let mut stream =
            tokio::time::timeout(Duration::from_secs(5), client(&tls).connect(name, tcp))
                .await
                .unwrap()
                .unwrap();
        stream
            .write_all(b"GET / HTTP/1.1\r\nHost: uniwrt.lan\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"), "{response}");
        assert!(response.ends_with("\r\n\r\n127.0.0.1"), "{response}");
    }
}
