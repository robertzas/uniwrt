//! The server against a client written to iperf 3.22's protocol, on loopback.

use std::{
    net::{IpAddr, Ipv4Addr},
    time::Duration,
};

use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
    time::{self, Instant},
};

use super::{
    Iperf3Error, Iperf3Summary, ServerConfig,
    proto::{self, Mode, Params, state},
    serve,
};

const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

fn config(allowed: IpAddr) -> ServerConfig {
    ServerConfig {
        allowed: vec![allowed],
        max_seconds: 60,
        max_streams: 8,
        handshake: Duration::from_secs(5),
    }
}

async fn server(config: ServerConfig) -> (u16, JoinHandle<Result<Iperf3Summary, Iperf3Error>>) {
    let listener = TcpListener::bind((LOOPBACK, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    (port, tokio::spawn(serve(listener, config)))
}

const COOKIE: &[u8; 37] = b"abcdefghijklmnopqrstuvwxyz0123456789\0";

async fn state_of(ctrl: &mut TcpStream) -> i8 {
    i8::from_be_bytes([ctrl.read_u8().await.unwrap()])
}

/// What the client saw: the server's results, and the bytes it sent and received on each stream.
struct Seen {
    results: Value,
    sent: Vec<u64>,
    received: Vec<u64>,
}

/// A test as iperf 3.22's client runs it: `parallel` streams for `millis`, in `mode` (seen from
/// the server).
async fn client(port: u16, mode: Mode, parallel: u32, millis: u64) -> Seen {
    let mut ctrl = TcpStream::connect((LOOPBACK, port)).await.unwrap();
    ctrl.write_all(COOKIE).await.unwrap();
    assert_eq!(state_of(&mut ctrl).await, state::PARAM_EXCHANGE);
    let mut params = json!({"tcp": true, "omit": 0, "time": 1, "num": 0, "blockcount": 0,
        "parallel": parallel, "len": 131_072, "pacing_timer": 1000, "client_version": "3.22"});
    match mode {
        Mode::Send => params["reverse"] = json!(true),
        Mode::Both => params["bidirectional"] = json!(true),
        Mode::Receive => {}
    }
    proto::write_json(&mut ctrl, &params).await.unwrap();
    assert_eq!(state_of(&mut ctrl).await, state::CREATE_STREAMS);
    let count = if mode == Mode::Both {
        parallel * 2
    } else {
        parallel
    };
    let mut handles = Vec::new();
    let until = Instant::now() + Duration::from_millis(millis);
    for index in 0..count {
        let mut stream = TcpStream::connect((LOOPBACK, port)).await.unwrap();
        stream.write_all(COOKIE).await.unwrap();
        // Seen from the client: it sends where the server receives.
        let sends = match mode {
            Mode::Receive => true,
            Mode::Send => false,
            Mode::Both => index < parallel,
        };
        handles.push(tokio::spawn(async move {
            let mut buf = vec![7_u8; 16 << 10];
            let mut bytes = 0_u64;
            while Instant::now() < until {
                let step = time::timeout_at(until, async {
                    if sends {
                        stream.write(&buf).await
                    } else {
                        stream.read(&mut buf).await
                    }
                });
                match step.await {
                    Ok(Ok(0) | Err(_)) | Err(_) => break,
                    Ok(Ok(n)) => bytes += n as u64,
                }
            }
            (sends, bytes, stream)
        }));
    }
    assert_eq!(state_of(&mut ctrl).await, state::TEST_START);
    assert_eq!(state_of(&mut ctrl).await, state::TEST_RUNNING);
    let (mut sent, mut received, mut streams) = (Vec::new(), Vec::new(), Vec::new());
    for handle in handles {
        let (sends, bytes, stream) = handle.await.unwrap();
        if sends {
            sent.push(bytes);
        } else {
            received.push(bytes);
        }
        streams.push(stream);
    }
    ctrl.write_all(&state::TEST_END.to_be_bytes())
        .await
        .unwrap();
    assert_eq!(state_of(&mut ctrl).await, state::EXCHANGE_RESULTS);
    let ours = json!({"cpu_util_total": 1.5, "cpu_util_user": 0.5, "cpu_util_system": 1.0,
        "sender_has_retransmits": 1, "streams": []});
    proto::write_json(&mut ctrl, &ours).await.unwrap();
    let results = proto::read_json(&mut ctrl, 1 << 20).await.unwrap();
    assert_eq!(state_of(&mut ctrl).await, state::DISPLAY_RESULTS);
    ctrl.write_all(&state::IPERF_DONE.to_be_bytes())
        .await
        .unwrap();
    Seen {
        results,
        sent,
        received,
    }
}

/// The fields iperf 3.22's `get_results` needs, and the streams' ids.
fn ids_of(results: &Value) -> Vec<u64> {
    for field in [
        "cpu_util_total",
        "cpu_util_user",
        "cpu_util_system",
        "sender_has_retransmits",
    ] {
        assert!(results[field].is_number(), "{field}: {results}");
    }
    results["streams"]
        .as_array()
        .unwrap()
        .iter()
        .map(|stream| {
            for field in ["id", "bytes", "retransmits", "jitter", "errors", "packets"] {
                assert!(stream[field].is_number(), "{field}: {stream}");
            }
            stream["id"].as_u64().unwrap()
        })
        .collect()
}

#[tokio::test]
async fn the_server_receives_what_the_client_sends() {
    let (port, server) = server(config(LOOPBACK)).await;
    let seen = client(port, Mode::Receive, 1, 300).await;
    let summary = server.await.unwrap().unwrap();
    assert_eq!(ids_of(&seen.results), [1]);
    assert_eq!(seen.results["sender_has_retransmits"], -1);
    let (received, sent) = summary.totals();
    assert_eq!(sent, 0);
    assert!(
        received > 0 && received <= seen.sent[0],
        "{received} of {}",
        seen.sent[0]
    );
    assert_eq!(seen.results["streams"][0]["bytes"], received);
    let seconds = summary.streams[0].seconds;
    assert!((0.25..2.0).contains(&seconds), "{seconds}");
}

#[tokio::test]
async fn in_reverse_the_server_sends_on_every_stream() {
    let (port, server) = server(config(LOOPBACK)).await;
    let seen = client(port, Mode::Send, 3, 300).await;
    let summary = server.await.unwrap().unwrap();
    assert_eq!(ids_of(&seen.results), [1, 3, 4]);
    assert_eq!(seen.results["sender_has_retransmits"], 0);
    assert!(summary.streams.iter().all(|stream| stream.sent));
    for (stream, received) in summary.streams.iter().zip(&seen.received) {
        assert!(
            *received > 0 && *received <= stream.bytes,
            "{received} of {}",
            stream.bytes
        );
    }
}

#[tokio::test]
async fn both_ways_the_first_half_of_the_streams_reaches_the_server() {
    let (port, server) = server(config(LOOPBACK)).await;
    let seen = client(port, Mode::Both, 2, 300).await;
    let summary = server.await.unwrap().unwrap();
    assert_eq!(ids_of(&seen.results), [1, 3, 4, 5]);
    let roles: Vec<bool> = summary.streams.iter().map(|stream| stream.sent).collect();
    assert_eq!(roles, [false, false, true, true]);
    assert_eq!(seen.sent.len(), 2);
    assert_eq!(seen.received.len(), 2);
    assert!(summary.streams.iter().all(|stream| stream.bytes > 0));
}

#[tokio::test]
async fn other_addresses_are_closed_unread() {
    let (port, server) = server(config(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)))).await;
    let mut ctrl = TcpStream::connect((LOOPBACK, port)).await.unwrap();
    ctrl.write_all(COOKIE).await.unwrap();
    let mut byte = [0; 1];
    let read = time::timeout(Duration::from_secs(5), ctrl.read(&mut byte)).await;
    assert!(matches!(read, Ok(Ok(0) | Err(_))), "{read:?}");
    assert!(!server.is_finished());
    server.abort();
}

#[tokio::test]
async fn what_the_server_doesnt_run_is_denied() {
    let (port, server) = server(config(LOOPBACK)).await;
    let mut ctrl = TcpStream::connect((LOOPBACK, port)).await.unwrap();
    ctrl.write_all(COOKIE).await.unwrap();
    assert_eq!(state_of(&mut ctrl).await, state::PARAM_EXCHANGE);
    proto::write_json(&mut ctrl, &json!({"udp": true, "time": 10, "parallel": 1}))
        .await
        .unwrap();
    assert_eq!(state_of(&mut ctrl).await, state::ACCESS_DENIED);
    let err = server.await.unwrap().unwrap_err();
    assert_eq!(err.to_string(), "refused: only TCP tests are taken");
}

#[tokio::test]
async fn a_stream_with_another_cookie_is_denied() {
    let (port, server) = server(config(LOOPBACK)).await;
    let mut ctrl = TcpStream::connect((LOOPBACK, port)).await.unwrap();
    ctrl.write_all(COOKIE).await.unwrap();
    assert_eq!(state_of(&mut ctrl).await, state::PARAM_EXCHANGE);
    proto::write_json(&mut ctrl, &json!({"tcp": true, "time": 10, "parallel": 1}))
        .await
        .unwrap();
    assert_eq!(state_of(&mut ctrl).await, state::CREATE_STREAMS);
    let mut other = TcpStream::connect((LOOPBACK, port)).await.unwrap();
    other.write_all(&[b'x'; 37]).await.unwrap();
    assert_eq!(state_of(&mut other).await, state::ACCESS_DENIED);
    // The client goes away before opening its stream.
    drop(ctrl);
    let err = server.await.unwrap().unwrap_err();
    assert!(matches!(err, Iperf3Error::Terminated), "{err}");
}

#[test]
fn parameters_are_bounded() {
    let parse = |value: Value| Params::parse(&value, 60, 8);
    let params = parse(
        json!({"tcp": true, "time": 10, "parallel": 4, "bidirectional": true,
        "len": 0, "omit": 1}),
    )
    .unwrap();
    assert_eq!(
        params,
        Params {
            mode: Mode::Both,
            parallel: 4,
            seconds: 10,
            omit: 1,
            block: 128 << 10,
        }
    );
    assert_eq!(
        (params.streams(), params.sends(3), params.sends(4)),
        (8, false, true)
    );
    assert_eq!(
        parse(json!({"time": 5, "reverse": 1})).unwrap().mode,
        Mode::Send
    );
    for refused in [
        json!({"time": 0}),
        json!({"time": 61}),
        json!({"time": 10, "parallel": 9}),
        json!({"time": 10, "parallel": 5, "bidirectional": true}),
        json!({"time": 10, "num": 1_000_000}),
        json!({"time": 10, "blockcount": 10}),
        json!({"time": 10, "sctp": true}),
        json!({"time": 10, "omit": 11}),
    ] {
        assert!(parse(refused.clone()).is_err(), "{refused}");
    }
    assert_eq!(
        (0..5).map(proto::stream_id).collect::<Vec<_>>(),
        [1, 3, 4, 5, 6]
    );
}
