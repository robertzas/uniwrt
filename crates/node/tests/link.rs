//! Supervised links against the in-process server: staying up, reconnecting, failing fast,
//! quarantine, the agent's own restarts and jump links.

// Test code throughout: clippy.toml allows unwrap in tests, but not in their helpers.
#![allow(clippy::unwrap_used)]

mod support;

use std::{sync::Arc, time::Duration};

use serde_json::{json, value::to_raw_value};
use support::{LONG, TestServer, server_with_key};
use tokio::sync::watch;
use uniwrt_core::SystemClock;
use uniwrt_node::{
    AgentEvent, AgentLaunch, AgentState, ControllerKey, Credential, Error, ExecRequest, LinkConfig,
    LinkState, NodeLink as _, RetryConfig, SshLink,
};

fn config(server: &TestServer, key: &ControllerKey, agent: Option<&[&str]>) -> LinkConfig {
    let mut config = LinkConfig::new(
        server.target(),
        server.host_key(),
        Credential::Key(key.clone()),
    );
    config.agent =
        agent.map(|argv| AgentLaunch::Command(argv.iter().map(|&arg| arg.to_owned()).collect()));
    config.retry = RetryConfig {
        min: Duration::from_millis(20),
        max: Duration::from_millis(100),
        ..RetryConfig::default()
    };
    config
}

fn spawn(config: LinkConfig) -> SshLink {
    SshLink::spawn(config, Arc::new(SystemClock))
}

async fn until(
    state: &mut watch::Receiver<LinkState>,
    want: impl Fn(&LinkState) -> bool,
) -> LinkState {
    let reached = tokio::time::timeout(LONG, async {
        state.wait_for(|state| want(state)).await.unwrap().clone()
    })
    .await;
    reached.unwrap_or_else(|_| panic!("still {:?}", *state.borrow()))
}

fn running(state: &LinkState) -> bool {
    matches!(
        state,
        LinkState::Up {
            agent: AgentState::Running(_),
            ..
        }
    )
}

#[tokio::test]
async fn a_link_comes_up_with_its_agent_and_serves_calls_events_and_commands() {
    let (server, key) = server_with_key(1).await;
    let link = spawn(config(&server, &key, Some(&["fake-agent"])));
    let LinkState::Up {
        agent: AgentState::Running(hello),
        ..
    } = until(&mut link.state(), running).await
    else {
        unreachable!()
    };
    assert_eq!(hello.agent, "fake");

    let mut events = link.events();
    let args = to_raw_value(&json!({"n": 1})).unwrap();
    assert_eq!(
        link.call("echo", &args, LONG).await.unwrap().get(),
        r#"{"n":1}"#
    );
    link.call("emit", &args, LONG).await.unwrap();
    assert!(matches!(
        events.recv().await.unwrap(),
        AgentEvent::Event { .. }
    ));
    let output = link.exec(ExecRequest::new(["echo", "hi"])).await.unwrap();
    assert_eq!(output.stdout, b"hi\n");

    link.shutdown().await;
    assert_eq!(*link.state().borrow(), LinkState::Stopped);
    assert!(link.session().is_none());
}

#[tokio::test]
async fn a_dropped_connection_comes_back_and_subscriptions_survive() {
    let (server, key) = server_with_key(1).await;
    let link = spawn(config(&server, &key, Some(&["fake-agent"])));
    let mut state = link.state();
    until(&mut state, running).await;
    let mut events = link.events();

    server.drop_connections().await;
    until(&mut state, |state| !running(state)).await;
    until(&mut state, running).await;
    assert_eq!(server.logins(), 2);

    let args = to_raw_value(&json!({})).unwrap();
    link.call("emit", &args, LONG).await.unwrap();
    assert!(matches!(
        events.recv().await.unwrap(),
        AgentEvent::Event { .. }
    ));
}

#[tokio::test]
async fn while_down_everything_fails_fast_and_attempts_back_off() {
    let (server, key) = server_with_key(1).await;
    let link = spawn(config(&server, &key, Some(&["fake-agent"])));
    let mut state = link.state();
    until(&mut state, running).await;

    server.refuse(true);
    server.drop_connections().await;
    let down = until(&mut state, |state| matches!(state, LinkState::Down { .. })).await;
    let LinkState::Down { error, .. } = down else {
        unreachable!()
    };
    assert_ne!(error, "");

    let args = to_raw_value(&json!({})).unwrap();
    let started = tokio::time::Instant::now();
    let err = link.call("echo", &args, LONG).await.unwrap_err();
    assert!(matches!(err, Error::Disconnected { .. }), "{err:?}");
    let err = link.exec(ExecRequest::new(["true"])).await.unwrap_err();
    assert!(matches!(err, Error::Disconnected { .. }), "{err:?}");
    assert!(started.elapsed() < Duration::from_millis(50));

    until(&mut state, |state| {
        matches!(state, LinkState::Connecting { attempt: 3.. })
    })
    .await;
    server.refuse(false);
    until(&mut state, running).await;
}

#[tokio::test]
async fn another_host_key_quarantines_the_link_without_retrying() {
    let (server, key) = server_with_key(1).await;
    let mut config = config(&server, &key, None);
    config.host_key = TestServer::start(2).await.host_key();
    let link = spawn(config);
    let quarantined = until(&mut link.state(), |state| {
        matches!(state, LinkState::Quarantined { .. })
    })
    .await;
    let LinkState::Quarantined { presented, .. } = quarantined else {
        unreachable!()
    };
    assert_eq!(presented, server.host_key());

    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(matches!(
        *link.state().borrow(),
        LinkState::Quarantined { .. }
    ));
    assert_eq!(server.logins(), 0);
}

#[tokio::test]
async fn a_failing_agent_degrades_the_link_but_commands_still_work() {
    let (server, key) = server_with_key(1).await;
    let link = spawn(config(
        &server,
        &key,
        Some(&["sh", "-c", "echo 'ucode: not found' >&2; exit 127"]),
    ));
    let mut state = link.state();
    let failed = until(&mut state, |state| {
        matches!(
            state,
            LinkState::Up {
                agent: AgentState::Failed { .. },
                ..
            }
        )
    })
    .await;
    let LinkState::Up {
        agent: AgentState::Failed { error, .. },
        ..
    } = failed
    else {
        unreachable!()
    };
    assert_eq!(
        error,
        "the agent is not running: the agent exited with exit code 127: ucode: not found"
    );

    let args = to_raw_value(&json!({})).unwrap();
    let err = link.call("echo", &args, LONG).await.unwrap_err();
    assert!(matches!(err, Error::AgentUnavailable { .. }), "{err:?}");
    let output = link
        .exec(ExecRequest::new(["echo", "still"]))
        .await
        .unwrap();
    assert_eq!(output.stdout, b"still\n");
    // The agent is retried on the same connection.
    until(&mut state, |state| {
        matches!(
            state,
            LinkState::Up {
                agent: AgentState::Starting,
                ..
            }
        )
    })
    .await;
    assert_eq!(server.logins(), 1);
}

#[tokio::test]
async fn an_agent_that_exits_is_restarted_on_the_same_connection() {
    let (server, key) = server_with_key(1).await;
    let link = spawn(config(&server, &key, Some(&["fake-agent"])));
    let mut state = link.state();
    until(&mut state, running).await;
    let args = to_raw_value(&json!({})).unwrap();
    let _ = link.call("exit", &args, LONG).await;
    until(&mut state, |state| !running(state)).await;
    until(&mut state, running).await;
    link.call("echo", &args, LONG).await.unwrap();
    assert_eq!(server.logins(), 1);
}

#[tokio::test]
async fn a_link_without_an_agent_runs_commands_only() {
    let (server, key) = server_with_key(1).await;
    let link = spawn(config(&server, &key, None));
    until(&mut link.state(), |state| {
        matches!(
            state,
            LinkState::Up {
                agent: AgentState::None,
                ..
            }
        )
    })
    .await;
    let args = to_raw_value(&json!({})).unwrap();
    let err = link.call("echo", &args, LONG).await.unwrap_err();
    assert!(
        matches!(&err, Error::AgentUnavailable { reason } if reason.contains("no agent")),
        "{err:?}"
    );
    link.exec(ExecRequest::new(["true"])).await.unwrap();
}

#[tokio::test]
async fn a_link_through_a_jump_link_follows_it_down_and_up() {
    let (jump_server, key) = server_with_key(1).await;
    let behind = TestServer::start(2).await;
    behind.authorize(&key);
    let jump = spawn(config(&jump_server, &key, None));
    let mut behind_config = config(&behind, &key, Some(&["fake-agent"]));
    behind_config.via = Some(jump.clone());
    let link = spawn(behind_config);
    let mut state = link.state();
    until(&mut state, running).await;

    jump_server.drop_connections().await;
    until(&mut state, |state| !running(state)).await;
    until(&mut state, running).await;
    assert_eq!((jump_server.logins(), behind.logins()), (2, 2));
    let output = link
        .exec(ExecRequest::new(["echo", "behind"]))
        .await
        .unwrap();
    assert_eq!(output.stdout, b"behind\n");
}

#[tokio::test]
async fn dropping_the_last_handle_stops_the_link() {
    let (server, key) = server_with_key(1).await;
    let link = spawn(config(&server, &key, None));
    let mut state = link.state();
    until(&mut state, |state| matches!(state, LinkState::Up { .. })).await;
    drop(link);
    until(&mut state, |state| *state == LinkState::Stopped).await;
}
