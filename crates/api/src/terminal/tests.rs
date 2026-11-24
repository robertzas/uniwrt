//! The terminal's WebSocket over a loopback socket, against the simulated fleet's pretend shell:
//! an admin's terminal from greeting to exit, resized and audited; the bounds, and the slot a
//! closed one frees; and what the upgrade refuses.

use std::{net::SocketAddr, time::Duration};

use axum::http::StatusCode;
use futures_util::{SinkExt as _, StreamExt as _};
use serde_json::{Value, json};
use tokio::{net::TcpStream, time::Instant};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, client_async,
    tungstenite::{self, Message, client::IntoClientRequest as _},
};
use uniwrt_core::NodeId;
use uniwrt_store::{AuditRepo as _, NodeRepo as _};

use super::PER_NODE;
use crate::testing::{Demo, ORIGIN, connect_h2};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

const PATIENCE: Duration = Duration::from_secs(10);

async fn connect(
    addr: SocketAddr,
    path: &str,
    cookie: Option<&str>,
    origin: &str,
) -> Result<Ws, tungstenite::Error> {
    let mut request = format!("ws://{addr}{path}").into_client_request()?;
    let headers = request.headers_mut();
    headers.insert("origin", origin.parse().unwrap());
    if let Some(cookie) = cookie {
        headers.insert("cookie", cookie.parse().unwrap());
    }
    let tcp = TcpStream::connect(addr).await.unwrap();
    let (ws, _) = client_async(request, MaybeTlsStream::Plain(tcp)).await?;
    Ok(ws)
}

fn status_of(err: tungstenite::Error) -> StatusCode {
    match err {
        tungstenite::Error::Http(response) => response.status(),
        other => panic!("expected an HTTP refusal, got {other}"),
    }
}

/// The next message that isn't a ping or pong.
async fn frame(ws: &mut Ws) -> Message {
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

async fn text(ws: &mut Ws) -> Value {
    match frame(ws).await {
        Message::Text(text) => serde_json::from_str(&text).unwrap(),
        other => panic!("expected a text message, got {other:?}"),
    }
}

/// The terminal's output until it ends with `suffix`.
async fn shown(ws: &mut Ws, suffix: &str) -> String {
    let mut shown = String::new();
    while !shown.ends_with(suffix) {
        match frame(ws).await {
            Message::Binary(data) => shown.push_str(&String::from_utf8_lossy(&data)),
            other => panic!("expected output, got {other:?} after {shown:?}"),
        }
    }
    shown
}

async fn name_of(demo: &Demo, id: NodeId) -> String {
    demo.store.node(id).await.unwrap().unwrap().name
}

#[tokio::test]
async fn browsers_on_http2_open_it_with_an_extended_connect() {
    let demo = Demo::start().await;
    let addr = demo.serve().await;
    let id = demo.ids[1];
    let path = format!("/v1/nodes/{id}/terminal");
    let mut ws = connect_h2(addr, &path, &demo.cookie).await;
    let opened = tokio::time::timeout(PATIENCE, ws.next())
        .await
        .expect("a message in time")
        .expect("the connection open")
        .unwrap();
    assert_eq!(opened, Message::text(json!({"type": "opened"}).to_string()));
}

#[tokio::test]
async fn an_admin_s_terminal_runs_the_shell_until_it_exits_and_is_audited() {
    let demo = Demo::start().await;
    let addr = demo.serve().await;
    let id = demo.ids[1];
    let name = name_of(&demo, id).await;
    let path = format!("/v1/nodes/{id}/terminal?cols=100&rows=30");
    let mut ws = connect(addr, &path, Some(&demo.cookie), ORIGIN)
        .await
        .unwrap();
    assert_eq!(text(&mut ws).await, json!({"type": "opened"}));
    let prompt = format!("root@{name}:~# ");
    assert!(shown(&mut ws, &prompt).await.contains("OpenWrt"));

    ws.send(Message::binary(&b"hostname\r"[..])).await.unwrap();
    assert_eq!(
        shown(&mut ws, &prompt).await,
        format!("hostname\r\n{name}\r\n{prompt}")
    );
    // A resize goes to the shell; one out of bounds, and a message not understood, are ignored.
    for resize in [
        json!({"type": "resize", "cols": 132, "rows": 43}),
        json!({"type": "resize", "cols": 0, "rows": 43}),
        json!({"type": "hello"}),
    ] {
        ws.send(Message::text(resize.to_string())).await.unwrap();
    }
    ws.send(Message::binary(&b"exit 4\r"[..])).await.unwrap();
    assert_eq!(shown(&mut ws, "\r\n").await, "exit 4\r\n");
    assert_eq!(
        text(&mut ws).await,
        json!({"type": "ended", "exit_code": 4, "reason": null})
    );
    let Message::Close(Some(close)) = frame(&mut ws).await else {
        panic!("expected a close frame")
    };
    assert_eq!(u16::from(close.code), 1000);

    let audit = demo.store.audit_log(2).await.unwrap();
    let details: Vec<&str> = audit
        .iter()
        .filter_map(|entry| entry.detail.as_deref())
        .collect();
    assert_eq!(
        details,
        [
            format!("terminal on {name} closed after 0 s: the shell exited with 4"),
            format!("terminal on {name} opened (100 × 30)"),
        ]
    );
    assert!(
        audit
            .iter()
            .all(|entry| entry.username.as_deref() == Some("admin"))
    );
}

#[tokio::test]
async fn terminals_are_bounded_per_node_and_a_closed_one_frees_its_slot() {
    let demo = Demo::start().await;
    let addr = demo.serve().await;
    let id = demo.ids[0];
    let path = format!("/v1/nodes/{id}/terminal");
    let mut open = Vec::new();
    for _ in 0..PER_NODE {
        let mut ws = connect(addr, &path, Some(&demo.cookie), ORIGIN)
            .await
            .unwrap();
        assert_eq!(text(&mut ws).await, json!({"type": "opened"}));
        open.push(ws);
    }
    let mut refused = connect(addr, &path, Some(&demo.cookie), ORIGIN)
        .await
        .unwrap();
    let ended = text(&mut refused).await;
    assert_eq!(ended["type"], "ended");
    assert_eq!(
        ended["reason"],
        format!("This device has {PER_NODE} terminals open already: close one first")
    );
    // Another node has room.
    let other = format!("/v1/nodes/{}/terminal", demo.ids[1]);
    let mut elsewhere = connect(addr, &other, Some(&demo.cookie), ORIGIN)
        .await
        .unwrap();
    assert_eq!(text(&mut elsewhere).await, json!({"type": "opened"}));

    let mut closed = open.pop().unwrap();
    closed.close(None).await.unwrap();
    let deadline = Instant::now() + PATIENCE;
    loop {
        let mut ws = connect(addr, &path, Some(&demo.cookie), ORIGIN)
            .await
            .unwrap();
        if text(&mut ws).await["type"] == "opened" {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the closed terminal kept its slot"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let audit = demo.store.audit_log(50).await.unwrap();
    assert!(audit.iter().any(|entry| {
        entry
            .detail
            .as_deref()
            .is_some_and(|detail| detail.ends_with(": the browser closed it"))
    }));
}

#[tokio::test]
async fn the_upgrade_needs_a_session_our_origin_a_node_and_a_size_in_bounds() {
    let demo = Demo::start().await;
    let addr = demo.serve().await;
    let path = format!("/v1/nodes/{}/terminal", demo.ids[0]);
    let refused = connect(addr, &path, None, ORIGIN).await.err().unwrap();
    assert_eq!(status_of(refused), StatusCode::UNAUTHORIZED);
    let refused = connect(addr, &path, Some(&demo.cookie), "https://evil.example")
        .await
        .err()
        .unwrap();
    assert_eq!(status_of(refused), StatusCode::FORBIDDEN);
    let refused = connect(addr, "/v1/nodes/999/terminal", Some(&demo.cookie), ORIGIN)
        .await
        .err()
        .unwrap();
    assert_eq!(status_of(refused), StatusCode::NOT_FOUND);
    for size in ["cols=0", "rows=1001", "cols=wide"] {
        let refused = connect(addr, &format!("{path}?{size}"), Some(&demo.cookie), ORIGIN)
            .await
            .err()
            .unwrap();
        assert_eq!(status_of(refused), StatusCode::BAD_REQUEST, "{size}");
    }
    // A plain GET is not an upgrade.
    let res = demo.send(demo.get(&path)).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert!(
        demo.store
            .audit_log(10)
            .await
            .unwrap()
            .iter()
            .all(|entry| { entry.action != uniwrt_model::user::AuditAction::Terminal })
    );
}
