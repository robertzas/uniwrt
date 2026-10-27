//! Sessions against the in-process server: trust on first use, authentication, commands,
//! uploads, jump hosts, disconnects and the agent over a real channel.

// Test code throughout: clippy.toml allows unwrap in tests, but not in their helpers.
#![allow(clippy::unwrap_used)]

mod support;

use std::{os::unix::fs::PermissionsExt as _, time::Duration};

use bytes::Bytes;
use serde_json::{json, value::to_raw_value};
use support::{LONG, TestServer, server_with_key};
use tokio::sync::broadcast;
use uniwrt_node::{
    Agent, AgentEvent, ControllerKey, Credential, Error, ExecRequest, ExitStatus, Output, Session,
    SshConfig, probe,
};
use zeroize::Zeroizing;

async fn connect(server: &TestServer, key: &ControllerKey) -> Session {
    Session::connect(
        &server.target(),
        None,
        &server.host_key(),
        &Credential::Key(key.clone()),
        &SshConfig::default(),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn a_probe_reads_the_host_key_without_logging_in() {
    let server = TestServer::start(1).await;
    let key = probe(&server.target(), None, &SshConfig::default())
        .await
        .unwrap();
    assert_eq!(key, server.host_key());
    assert!(key.fingerprint().starts_with("SHA256:"));
    assert_eq!(server.logins(), 0);
}

#[tokio::test]
async fn another_host_key_than_the_pinned_one_is_a_mismatch() {
    let (server, key) = server_with_key(1).await;
    let other = TestServer::start(2).await.host_key();
    let err = Session::connect(
        &server.target(),
        None,
        &other,
        &Credential::Key(key),
        &SshConfig::default(),
    )
    .await
    .unwrap_err();
    let Error::HostKeyMismatch {
        presented, pinned, ..
    } = err
    else {
        panic!("expected a mismatch, got {err:?}");
    };
    assert_eq!((presented, pinned), (server.host_key(), other));
    assert_eq!(server.logins(), 0);
}

#[tokio::test]
async fn keys_and_passwords_log_in_and_wrong_ones_are_rejected() {
    let (server, key) = server_with_key(1).await;
    server.set_password("hunter2");
    let config = SshConfig::default();
    let target = server.target();
    let pinned = server.host_key();

    connect(&server, &key).await;
    let password = Credential::Password(Zeroizing::new("hunter2".to_owned()));
    Session::connect(&target, None, &pinned, &password, &config)
        .await
        .unwrap();

    let stranger = Credential::Key(ControllerKey::generate().unwrap());
    let err = Session::connect(&target, None, &pinned, &stranger, &config)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            Error::AuthRejected {
                method: "publickey",
                ..
            }
        ),
        "{err:?}"
    );
    let wrong = Credential::Password(Zeroizing::new("hunter3".to_owned()));
    let err = Session::connect(&target, None, &pinned, &wrong, &config)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            Error::AuthRejected {
                method: "password",
                ..
            }
        ),
        "{err:?}"
    );
    assert_eq!(server.logins(), 2);
}

#[tokio::test]
async fn nobody_listening_and_a_silent_listener_fail_cleanly() {
    let server = TestServer::start(1).await;
    let target = server.target();
    drop(server);
    let key = TestServer::start(9).await.host_key();
    let credential = Credential::Key(ControllerKey::generate().unwrap());
    let err = Session::connect(&target, None, &key, &credential, &SshConfig::default())
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Connect { .. }), "{err:?}");

    // Accepts TCP but never speaks SSH.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut target = target;
    target.port = listener.local_addr().unwrap().port();
    let config = SshConfig {
        connect_timeout: Duration::from_millis(200),
        ..SshConfig::default()
    };
    let err = Session::connect(&target, None, &key, &credential, &config)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Timeout { .. }), "{err:?}");
}

#[tokio::test]
async fn commands_run_with_quoted_arguments_and_report_output_and_status() {
    let (server, key) = server_with_key(1).await;
    let session = connect(&server, &key).await;
    let tricky = ["a b", "it's", "$HOME", "*", "", "line\nbreak", "; rm -rf /"];
    let mut argv = vec!["printf", r"[%s]\n"];
    argv.extend(tricky);
    let output = session.exec(&ExecRequest::new(argv)).await.unwrap();
    assert_eq!(output.status, ExitStatus::Code(0));
    let expected = tricky.map(|arg| format!("[{arg}]\n")).concat();
    assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);

    let output = session
        .exec(&ExecRequest::new(["sh", "-c", "echo oops >&2; exit 3"]))
        .await
        .unwrap();
    assert_eq!(output.status, ExitStatus::Code(3));
    assert_eq!(output.stderr, b"oops\n");
    let err = output.check("`sh`").unwrap_err();
    assert!(
        matches!(&err, Error::Failed { stderr, .. } if stderr == "oops"),
        "{err:?}"
    );
}

#[tokio::test]
async fn stdin_round_trips_concurrently_with_output_and_limits_hold() {
    let (server, key) = server_with_key(1).await;
    let session = connect(&server, &key).await;
    // More than the channel window, so writing and reading must overlap.
    let data: Vec<u8> = (0..3_000_000_u32).map(|n| n.to_le_bytes()[0]).collect();
    let output = session
        .exec(&ExecRequest::new(["cat"]).stdin(data.clone()))
        .await
        .unwrap();
    assert_eq!(output.stdout.len(), data.len());
    // Not assert_eq: a mismatch would print megabytes.
    assert!(output.stdout.iter().eq(&data), "the data came back changed");

    let err = session
        .exec(&ExecRequest::new(["head", "-c", "100000", "/dev/zero"]).max_output(1_000))
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::Overflow { limit: 1_000, .. }),
        "{err:?}"
    );

    let err = session
        .exec(&ExecRequest::new(["sleep", "5"]).timeout(Duration::from_millis(100)))
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Timeout { .. }), "{err:?}");
    // The session survives both.
    let output = session.exec(&ExecRequest::new(["true"])).await.unwrap();
    assert!(output.status.success());
}

#[tokio::test]
async fn many_commands_share_one_connection() {
    let (server, key) = server_with_key(1).await;
    let session = connect(&server, &key).await;
    let mut runs = tokio::task::JoinSet::new();
    for n in 0..20 {
        let session = session.clone();
        runs.spawn(async move {
            let output = session
                .exec(&ExecRequest::new(["echo", &n.to_string()]))
                .await
                .unwrap();
            assert_eq!(output.stdout, format!("{n}\n").into_bytes());
        });
    }
    while let Some(run) = runs.join_next().await {
        run.unwrap();
    }
    assert_eq!(server.logins(), 1);
}

#[tokio::test]
async fn a_stream_is_interactive() {
    let (server, key) = server_with_key(1).await;
    let session = connect(&server, &key).await;
    let mut stream = session.stream(&["cat"]).await.unwrap();
    for word in ["one\n", "two\n"] {
        stream.write(word).await.unwrap();
        assert_eq!(
            stream.next().await.unwrap().unwrap(),
            Output::Stdout(Bytes::from(word))
        );
    }
    stream.close_stdin();
    assert_eq!(
        stream.next().await.unwrap().unwrap(),
        Output::Exit(ExitStatus::Code(0))
    );
    assert!(stream.next().await.is_none());
}

#[tokio::test]
async fn uploads_land_atomically_with_their_mode() {
    let (server, key) = server_with_key(1).await;
    let session = connect(&server, &key).await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("it's a file");
    let path_text = path.to_str().unwrap();

    session
        .upload(path_text, Bytes::from_static(b"first"), 0o640)
        .await
        .unwrap();
    session
        .upload(path_text, Bytes::from_static(b"second"), 0o600)
        .await
        .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"second");
    let mode = std::fs::metadata(&path).unwrap().permissions().mode();
    assert_eq!(mode & 0o7777, 0o600);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);

    let err = session
        .upload("/nonexistent-dir/x", Bytes::from_static(b"x"), 0o600)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Failed { .. }), "{err:?}");
}

#[tokio::test]
async fn a_jump_session_tunnels_to_a_node_behind_it() {
    let (jump, key) = server_with_key(1).await;
    let behind = TestServer::start(2).await;
    behind.authorize(&key);
    let jump_session = connect(&jump, &key).await;

    let credential = Credential::Key(key);
    let config = SshConfig::default();
    let found = probe(&behind.target(), Some(&jump_session), &config)
        .await
        .unwrap();
    assert_eq!(found, behind.host_key());
    let session = Session::connect(
        &behind.target(),
        Some(&jump_session),
        &behind.host_key(),
        &credential,
        &config,
    )
    .await
    .unwrap();
    let output = session
        .exec(&ExecRequest::new(["echo", "behind"]))
        .await
        .unwrap();
    assert_eq!(output.stdout, b"behind\n");
    assert_eq!((jump.logins(), behind.logins()), (1, 1));

    // The pin is still checked through the tunnel.
    let err = Session::connect(
        &behind.target(),
        Some(&jump_session),
        &jump.host_key(),
        &credential,
        &config,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, Error::HostKeyMismatch { .. }), "{err:?}");
}

#[tokio::test]
async fn a_dropped_connection_ends_the_session_and_fails_commands_in_flight() {
    let (server, key) = server_with_key(1).await;
    let session = connect(&server, &key).await;
    let running = {
        let session = session.clone();
        tokio::spawn(async move { session.exec(&ExecRequest::new(["sleep", "5"])).await })
    };
    tokio::time::sleep(Duration::from_millis(200)).await;
    server.drop_connections().await;

    let reason = tokio::time::timeout(LONG, session.closed()).await.unwrap();
    assert_ne!(reason, "");
    assert!(session.is_closed());
    let err = running.await.unwrap().unwrap_err();
    assert!(matches!(err, Error::Disconnected { .. }), "{err:?}");
    let err = session.exec(&ExecRequest::new(["true"])).await.unwrap_err();
    assert!(
        matches!(err, Error::Ssh { .. } | Error::Disconnected { .. }),
        "{err:?}"
    );
}

#[tokio::test]
async fn the_agent_runs_over_a_channel() {
    let (server, key) = server_with_key(1).await;
    let session = connect(&server, &key).await;
    let (events, mut received) = broadcast::channel(8);
    let stream = session.stream(&["fake-agent"]).await.unwrap();
    let agent = Agent::start(stream, events.clone(), LONG).await.unwrap();
    assert_eq!(agent.hello().agent, "fake");

    let args = to_raw_value(&json!({"n": 1})).unwrap();
    assert_eq!(
        agent.call("echo", &args, LONG).await.unwrap().get(),
        r#"{"n":1}"#
    );
    agent.call("emit", &args, LONG).await.unwrap();
    assert!(
        matches!(received.recv().await.unwrap(), AgentEvent::Event { topic, .. } if topic == "test.event")
    );
    let err = agent.call("nope", &args, LONG).await.unwrap_err();
    assert!(matches!(err, Error::Remote { code, .. } if code == "unknown_op"));

    let _ = agent.call("exit", &args, LONG).await;
    assert_eq!(
        agent.closed().await,
        "the agent exited with exit code 3: bye"
    );

    let stream = session.stream(&["fake-agent", "--proto-99"]).await.unwrap();
    let err = Agent::start(stream, events, LONG).await.unwrap_err();
    assert!(matches!(err, Error::Protocol(_)), "{err:?}");
}
