//! The multiplexer against an in-memory fake agent.

use std::time::Duration;

use bytes::Bytes;
use serde_json::{Value, json, value::RawValue};
use tokio::{sync::broadcast, task::JoinSet};
use uniwrt_core::Timestamp;

use super::Agent;
use crate::{
    Error,
    exec::{ExitStatus, Output, StreamSource, pipe},
    lines::LineBuf,
    proto::AgentEvent,
};

const HELLO: &str = r#"{"hello":{"proto":1,"agent":"test","release":"25.12.6","caps":{}}}"#;
const LONG: Duration = Duration::from_secs(5);

fn line(value: &Value) -> Output {
    Output::Stdout(Bytes::from(format!("{value}\n")))
}

fn args(value: &Value) -> Box<RawValue> {
    serde_json::value::to_raw_value(value).unwrap()
}

/// Says `hello`, then answers by op: `echo` returns its args, `fail` an error, `emit` pushes an
/// event and a tick first, `silent` never answers, `exit` prints to stderr and exits with 3.
async fn fake(mut source: StreamSource, hello: &str) {
    if !hello.is_empty() {
        source
            .output
            .push(Output::Stdout(Bytes::from(format!("{hello}\n"))));
    }
    let mut lines = LineBuf::new(1 << 20);
    while let Some(data) = source.stdin.next().await {
        lines.push(&data);
        while let Some(request) = lines.next_line().unwrap() {
            let request: Value = serde_json::from_slice(request).unwrap();
            let id = &request["id"];
            let answer = match request["op"].as_str().unwrap() {
                "echo" => json!({"id": id, "ok": request["args"]}),
                "fail" => json!({"id": id, "err": {"code": "boom", "msg": "on purpose"}}),
                "emit" => {
                    source.output.push(line(
                        &json!({"ev": "test.event", "ts": 5, "data": request["args"]}),
                    ));
                    source
                        .output
                        .push(Output::Stdout(Bytes::from_static(b"garbage\n")));
                    source.output.push(line(&json!({"tick": {"system": {}}})));
                    json!({"id": id, "ok": {}})
                }
                "silent" => continue,
                "exit" => {
                    source
                        .output
                        .push(Output::Stderr(Bytes::from_static(b"bye\n")));
                    source.output.push(Output::Exit(ExitStatus::Code(3)));
                    return;
                }
                op => panic!("unexpected op {op}"),
            };
            source.output.push(line(&answer));
        }
    }
}

async fn start_with(hello: &'static str) -> (crate::Result<Agent>, broadcast::Sender<AgentEvent>) {
    let (stream, source) = pipe("`agent`", 1 << 20);
    tokio::spawn(fake(source, hello));
    let (events, _) = broadcast::channel(16);
    let agent = Agent::start(stream, events.clone(), Duration::from_millis(200)).await;
    (agent, events)
}

async fn start() -> (Agent, broadcast::Sender<AgentEvent>) {
    let (agent, events) = start_with(HELLO).await;
    (agent.unwrap(), events)
}

#[tokio::test]
async fn calls_are_answered_by_id_even_when_concurrent() {
    let (agent, _) = start().await;
    assert_eq!(agent.hello().agent, "test");
    assert_eq!(agent.hello().release.as_deref(), Some("25.12.6"));

    let mut calls = JoinSet::new();
    for n in 0..50 {
        let agent = agent.clone();
        calls.spawn(async move {
            let answer = agent
                .call("echo", &args(&json!({"n": n})), LONG)
                .await
                .unwrap();
            assert_eq!(answer.get(), format!(r#"{{"n":{n}}}"#));
        });
    }
    while let Some(done) = calls.join_next().await {
        done.unwrap();
    }
    assert!(agent.0.calls.table().waiting.is_empty());
}

#[tokio::test]
async fn an_error_answer_names_the_op() {
    let (agent, _) = start().await;
    let err = agent
        .call("fail", &args(&json!({})), LONG)
        .await
        .unwrap_err();
    assert!(
        matches!(&err, Error::Remote { op, code, msg } if op == "fail" && code == "boom" && msg == "on purpose"),
        "{err:?}"
    );
}

#[tokio::test]
async fn events_and_ticks_are_broadcast_and_garbage_is_skipped() {
    let (agent, events) = start().await;
    let mut receiver = events.subscribe();
    agent
        .call("emit", &args(&json!({"mac": "aa"})), LONG)
        .await
        .unwrap();
    let AgentEvent::Event { topic, ts, data } = receiver.recv().await.unwrap() else {
        panic!("expected an event");
    };
    assert_eq!(
        (topic.as_str(), ts.map(Timestamp::as_unix_ms)),
        ("test.event", Some(5))
    );
    assert_eq!(data.get(), r#"{"mac":"aa"}"#);
    assert!(matches!(
        receiver.recv().await.unwrap(),
        AgentEvent::Tick(_)
    ));
    // The agent still answers after the malformed line.
    agent.call("echo", &args(&json!(1)), LONG).await.unwrap();
}

#[tokio::test]
async fn a_call_that_times_out_or_is_dropped_leaves_nothing_behind() {
    let (agent, _) = start().await;
    let err = agent
        .call("silent", &args(&json!({})), Duration::from_millis(20))
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Timeout { .. }), "{err:?}");

    let silent = args(&json!({}));
    let call = agent.call("silent", &silent, LONG);
    tokio::select! {
        _ = call => panic!("silent answered"),
        () = tokio::time::sleep(Duration::from_millis(20)) => {}
    }
    assert!(agent.0.calls.table().waiting.is_empty());
    agent.call("echo", &args(&json!(2)), LONG).await.unwrap();
}

#[tokio::test]
async fn when_the_agent_exits_calls_in_flight_and_later_fail_with_the_reason() {
    let (agent, _) = start().await;
    let silent = {
        let agent = agent.clone();
        tokio::spawn(async move { agent.call("silent", &args(&json!({})), LONG).await })
    };
    tokio::time::sleep(Duration::from_millis(20)).await;
    let _ = agent.call("exit", &args(&json!({})), LONG).await;

    let reason = agent.closed().await;
    assert_eq!(reason, "the agent exited with exit code 3: bye");
    assert!(agent.is_closed());
    let err = silent.await.unwrap().unwrap_err();
    assert!(
        matches!(&err, Error::AgentUnavailable { reason: r } if *r == reason),
        "{err:?}"
    );
    let err = agent
        .call("echo", &args(&json!(3)), LONG)
        .await
        .unwrap_err();
    assert!(
        matches!(&err, Error::AgentUnavailable { reason: r } if *r == reason),
        "{err:?}"
    );
}

#[tokio::test]
async fn a_wrong_protocol_a_missing_hello_or_silence_fails_the_start() {
    let (agent, _) = start_with(r#"{"hello":{"proto":99,"agent":"x"}}"#).await;
    let err = agent.unwrap_err();
    assert!(
        matches!(&err, Error::Protocol(msg) if msg.contains("protocol 99")),
        "{err:?}"
    );

    let (agent, _) = start_with(r#"{"tick":{}}"#).await;
    assert!(matches!(agent.unwrap_err(), Error::Protocol(_)));

    let (agent, _) = start_with("").await;
    assert!(matches!(agent.unwrap_err(), Error::Timeout { .. }));
}

#[tokio::test]
async fn an_agent_that_dies_before_its_hello_explains_why() {
    let (stream, source) = pipe("`agent`", 1 << 20);
    source.output.push(Output::Stderr(Bytes::from_static(
        b"sh: ucode: not found\n",
    )));
    source.output.push(Output::Exit(ExitStatus::Code(127)));
    drop(source);
    let (events, _) = broadcast::channel(1);
    let err = Agent::start(stream, events, LONG).await.unwrap_err();
    assert!(
        matches!(&err, Error::AgentUnavailable { reason } if reason == "the agent exited with exit code 127: sh: ucode: not found"),
        "{err:?}"
    );
}
