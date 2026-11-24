//! A WebSocket opened as browsers open one on an HTTP/2 connection: an extended CONNECT
//! (RFC 8441), here over cleartext HTTP/2 with prior knowledge.

use std::net::SocketAddr;

use axum::http::{StatusCode, Version};
use hyper::{ext::Protocol, upgrade::Upgraded};
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio::net::TcpStream;
use tokio_tungstenite::{WebSocketStream, tungstenite::protocol::Role};

use super::ORIGIN;

/// Opens the WebSocket at `path` with the session's `cookie`, asserting that it upgrades.
pub(crate) async fn connect_h2(
    addr: SocketAddr,
    path: &str,
    cookie: &str,
) -> WebSocketStream<TokioIo<Upgraded>> {
    let tcp = TcpStream::connect(addr).await.unwrap();
    let (mut sender, connection) =
        hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(tcp))
            .await
            .unwrap();
    tokio::spawn(connection);
    let mut request = axum::http::Request::connect(format!("http://{addr}{path}"))
        .version(Version::HTTP_2)
        .header("sec-websocket-version", "13")
        .header("cookie", cookie)
        .header("origin", ORIGIN)
        .body(String::new())
        .unwrap();
    request
        .extensions_mut()
        .insert(Protocol::from_static("websocket"));
    let response = sender.send_request(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let upgraded = hyper::upgrade::on(response).await.unwrap();
    WebSocketStream::from_raw_socket(TokioIo::new(upgraded), Role::Client, None).await
}
