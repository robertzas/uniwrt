//! The WS hub over a loopback socket, against the simulated fleet: the upgrade's checks,
//! snapshots then deltas, interest while topics are held, the rate of deltas, and closing.

use std::{net::SocketAddr, time::Duration};

use axum::http::{StatusCode, Version};
use futures_util::{SinkExt as _, StreamExt as _};
use hyper::{ext::Protocol, upgrade::Upgraded};
use hyper_util::rt::{TokioExecutor, TokioIo};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
    time::Instant,
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, client_async,
    tungstenite::{
        self, Message,
        client::IntoClientRequest as _,
        protocol::{Role, frame::coding::CloseCode},
    },
};
use uniwrt_core::NodeId;
use uniwrt_engine::Interests;

use super::conn::{CLOSE_SESSION_ENDED, FLUSH_EVERY, SESSION_CHECK_EVERY};
use crate::testing::{Demo, ORIGIN, post};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// What a WebSocket runs over: TCP, or an HTTP/2 stream.
trait Stream: AsyncRead + AsyncWrite + Unpin {}

impl<S: AsyncRead + AsyncWrite + Unpin> Stream for S {}

const PATIENCE: Duration = Duration::from_secs(10);

async fn connect(
    addr: SocketAddr,
    cookie: Option<&str>,
    origin: &str,
) -> Result<Ws, tungstenite::Error> {
    let mut request = format!("ws://{addr}/v1/ws").into_client_request()?;
    let headers = request.headers_mut();
    headers.insert("origin", origin.parse().unwrap());
    if let Some(cookie) = cookie {
        headers.insert("cookie", cookie.parse().unwrap());
    }
    let tcp = TcpStream::connect(addr).await.unwrap();
    let (ws, _) = client_async(request, MaybeTlsStream::Plain(tcp)).await?;
    Ok(ws)
}

/// Opens the hub as browsers do on an HTTP/2 connection: an extended CONNECT (RFC 8441), here
/// over cleartext HTTP/2 with prior knowledge.
async fn connect_h2(addr: SocketAddr, cookie: &str) -> WebSocketStream<TokioIo<Upgraded>> {
    let tcp = TcpStream::connect(addr).await.unwrap();
    let (mut sender, connection) =
        hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(tcp))
            .await
            .unwrap();
    tokio::spawn(connection);
    let mut request = axum::http::Request::connect(format!("http://{addr}/v1/ws"))
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

async fn open(demo: &Demo) -> Ws {
    let addr = demo.serve().await;
    connect(addr, Some(&demo.cookie), ORIGIN).await.unwrap()
}

async fn sub<S: Stream>(ws: &mut WebSocketStream<S>, topics: &[&str]) {
    let text = json!({ "sub": topics }).to_string();
    ws.send(Message::text(text)).await.unwrap();
}

/// The next frame that is not a ping or pong.
async fn frame<S: Stream>(ws: &mut WebSocketStream<S>) -> Message {
    loop {
        let message = tokio::time::timeout(PATIENCE, ws.next())
            .await
            .expect("a message in time")
            .expect("the connection open")
            .unwrap();
        if !matches!(message, Message::Ping(_) | Message::Pong(_)) {
            return message;
        }
    }
}

async fn next<S: Stream>(ws: &mut WebSocketStream<S>) -> Value {
    match frame(ws).await {
        Message::Text(text) => serde_json::from_str(&text).unwrap(),
        other => panic!("expected a text message, got {other:?}"),
    }
}

/// The first message that `wanted` accepts, within [`PATIENCE`].
async fn until(ws: &mut Ws, wanted: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + PATIENCE;
    loop {
        assert!(Instant::now() < deadline, "no such message in time");
        let message = next(ws).await;
        if wanted(&message) {
            return message;
        }
    }
}

async fn interests_become(demo: &Demo, node: NodeId, wanted: Interests) {
    let deadline = Instant::now() + PATIENCE;
    while demo.engine.interests(node) != Some(wanted) {
        assert!(
            Instant::now() < deadline,
            "interests never became {wanted:?}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn status_of(err: tungstenite::Error) -> StatusCode {
    match err {
        tungstenite::Error::Http(response) => response.status(),
        other => panic!("expected an HTTP refusal, got {other}"),
    }
}

#[tokio::test]
async fn the_upgrade_needs_a_session_and_our_origin() {
    let demo = Demo::start().await;
    let addr = demo.serve().await;
    let refused = connect(addr, None, ORIGIN).await.err().unwrap();
    assert_eq!(status_of(refused), StatusCode::UNAUTHORIZED);
    let refused = connect(addr, Some(&demo.cookie), "https://evil.example")
        .await
        .err()
        .unwrap();
    assert_eq!(status_of(refused), StatusCode::FORBIDDEN);
    assert!(connect(addr, Some(&demo.cookie), ORIGIN).await.is_ok());
    // A plain GET is not an upgrade.
    let res = demo.send(demo.get("/v1/ws")).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(res.headers()["content-type"], "application/problem+json");
}

#[tokio::test]
async fn browsers_on_http2_open_it_with_an_extended_connect() {
    let demo = Demo::start().await;
    let addr = demo.serve().await;
    let mut ws = connect_h2(addr, &demo.cookie).await;
    sub(&mut ws, &["fleet"]).await;
    let snapshot = next(&mut ws).await;
    assert_eq!(snapshot["type"], "fleet_snapshot");
    assert_eq!(snapshot["nodes"].as_array().unwrap().len(), 4);
}

#[tokio::test]
async fn topics_answer_with_a_snapshot_then_deltas_and_hold_focus() {
    let demo = Demo::start().await;
    let mut ws = open(&demo).await;
    let gateway = demo.ids[0];
    let topic = format!("node:{gateway}");
    sub(&mut ws, &["fleet", &topic]).await;
    let fleet = next(&mut ws).await;
    assert_eq!(fleet["type"], "fleet_snapshot");
    assert_eq!(fleet["topic"], "fleet");
    assert_eq!(fleet["nodes"].as_array().unwrap().len(), 4);
    assert_eq!(fleet["nodes"][0]["role"], "gateway");
    let node = next(&mut ws).await;
    assert_eq!(node["type"], "node_snapshot");
    assert_eq!(node["topic"], topic.as_str());
    assert_eq!(node["node"]["node"]["id"], gateway.get());
    interests_become(&demo, gateway, Interests { focus: 1, logs: 0 }).await;

    until(&mut ws, |message| {
        message["type"] == "fleet_delta" && message["nodes"][0]["status"]["state"] == "online"
    })
    .await;
    let delta = until(&mut ws, |message| {
        message["type"] == "node_delta" && message.get("facts").is_some()
    })
    .await;
    assert_eq!(delta["topic"], topic.as_str());

    // Leaving the topic ends the focus; holding the fleet sends no snapshot again.
    sub(&mut ws, &["fleet"]).await;
    interests_become(&demo, gateway, Interests::default()).await;

    sub(&mut ws, &["fleet", "topology", "node:999"]).await;
    let errors = [
        until(&mut ws, |message| message["type"] == "error").await,
        next(&mut ws).await,
    ];
    assert_eq!(errors[0]["topic"], "node:999");
    assert_eq!(errors[1]["topic"], "topology");
    ws.send(Message::text("hello")).await.unwrap();
    let error = until(&mut ws, |message| message["type"] == "error").await;
    assert_eq!(error["topic"], Value::Null);
}

#[tokio::test]
async fn a_logs_topic_follows_the_log_while_held() {
    let demo = Demo::start().await;
    let mut ws = open(&demo).await;
    let gateway = demo.ids[0];
    let topic = format!("logs?node={gateway}&min=info");
    sub(&mut ws, &[&topic]).await;
    let snapshot = next(&mut ws).await;
    assert_eq!(snapshot["type"], "log_snapshot");
    interests_become(&demo, gateway, Interests { focus: 0, logs: 1 }).await;
    // Following starts with the agent's backfill.
    let delta = until(&mut ws, |message| message["type"] == "log_delta").await;
    let entries = delta["entries"].as_array().unwrap();
    assert_ne!(entries.len(), 0);
    assert!(entries.iter().all(|entry| entry["severity"] != "debug"));
    sub(&mut ws, &[]).await;
    interests_become(&demo, gateway, Interests::default()).await;
}

#[tokio::test]
async fn deltas_go_out_at_most_every_flush_per_topic() {
    let demo = Demo::start().await;
    let mut ws = open(&demo).await;
    let topics: Vec<String> = demo.ids.iter().map(|id| format!("node:{id}")).collect();
    let mut names: Vec<&str> = topics.iter().map(String::as_str).collect();
    names.push("fleet");
    sub(&mut ws, &names).await;
    let mut last = std::collections::BTreeMap::new();
    let mut deltas = 0;
    let end = Instant::now() + Duration::from_secs(3);
    while Instant::now() < end {
        let message = next(&mut ws).await;
        if !message["type"].as_str().unwrap().ends_with("_delta") {
            continue;
        }
        deltas += 1;
        let now = Instant::now();
        let topic = message["topic"].as_str().unwrap().to_owned();
        if let Some(before) = last.insert(topic.clone(), now) {
            // Scheduling may bunch two flushes' sends a little, never by half a flush.
            let gap = now - before;
            assert!(gap >= FLUSH_EVERY / 2, "{topic}: deltas {gap:?} apart");
        }
    }
    // The sim's nodes come online and collect in these seconds.
    assert!(deltas > 5, "only {deltas} deltas");
}

#[tokio::test]
async fn sockets_close_when_the_server_stops() {
    let demo = Demo::start().await;
    let mut ws = open(&demo).await;
    sub(&mut ws, &["fleet"]).await;
    next(&mut ws).await;
    demo.api.close_sockets();
    let Message::Close(Some(close)) = frame(&mut ws).await else {
        panic!("expected a close frame");
    };
    assert_eq!(close.code, CloseCode::Away);
    // A connection opened later closes at once.
    let mut late = open(&demo).await;
    let Message::Close(Some(close)) = frame(&mut late).await else {
        panic!("expected a close frame");
    };
    assert_eq!(close.code, CloseCode::Away);
}

#[tokio::test(start_paused = true)]
async fn sockets_close_when_their_session_ends() {
    let demo = Demo::start().await;
    let mut ws = open(&demo).await;
    sub(&mut ws, &["fleet"]).await;
    next(&mut ws).await;
    let res = demo
        .send(post("/v1/auth/logout", &json!({}), Some(&demo.cookie)))
        .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let close = async {
        loop {
            if let Message::Close(frame) = ws.next().await.unwrap().unwrap() {
                break frame.unwrap();
            }
        }
    };
    let frame = tokio::time::timeout(SESSION_CHECK_EVERY * 2, close)
        .await
        .expect("closed within two session checks");
    assert_eq!(frame.code, CloseCode::from(CLOSE_SESSION_ENDED));
}
