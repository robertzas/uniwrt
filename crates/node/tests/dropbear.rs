//! The transport against dropbear on real OpenWrt (TESTING.md, Transport). Ignored by default:
//! `just test-dropbear` starts an OpenWrt rootfs container per release and runs these against
//! it, with `UNIWRT_NODE=<host:port>` and `UNIWRT_NODE_PASSWORD=<root password>`.

// Test code throughout: clippy.toml allows unwrap in tests, but not in their helpers.
#![allow(clippy::unwrap_used)]

use std::{sync::Arc, time::Duration};

use bytes::Bytes;
use serde_json::{json, value::to_raw_value};
use uniwrt_core::SystemClock;
use uniwrt_node::{
    AUTHORIZED_KEY_COMMENT, AgentLaunch, AgentState, Credential, Error, ExecRequest, ExitStatus,
    HostKey, LinkConfig, LinkState, NodeLink as _, Output, Session, SshConfig, SshLink, Target,
    banner, probe,
};

mod openwrt;

use openwrt::{adopt, run};

const LONG: Duration = Duration::from_secs(10);

/// A minimal agent in ucode: says hello, then echoes each request's args.
const ECHO_AGENT: &str = r#"
import { stdin, stdout } from "fs";
print(sprintf("%J\n", { hello: { proto: 1, agent: "echo", release: "test" } }));
stdout.flush();
for (let line = stdin.read("line"); length(line); line = stdin.read("line")) {
	let request = json(line);
	print(sprintf("%J\n", { id: request.id, ok: request.args }));
	stdout.flush();
}
"#;

#[tokio::test]
#[ignore = "needs dropbear in an OpenWrt container: just test-dropbear"]
async fn adoption_installs_the_controller_key_and_logs_in_with_it() {
    let (session, node) = adopt().await;
    // Discovery tells OpenWrt apart by its SSH banner.
    let line = banner(&node.target, LONG).await.unwrap().unwrap();
    assert!(line.starts_with("SSH-2.0-dropbear"), "{line}");
    let release = run(&session, &["cat", "/etc/openwrt_release"]).await;
    assert!(release.contains("DISTRIB_ID='OpenWrt'"), "{release}");
    let keys = run(&session, &["cat", "/etc/dropbear/authorized_keys"]).await;
    let line = node.key.authorized_key().unwrap();
    assert!(keys.lines().any(|key| key == line), "{keys}");
    assert!(line.ends_with(AUTHORIZED_KEY_COMMENT));
}

#[tokio::test]
#[ignore = "needs dropbear in an OpenWrt container: just test-dropbear"]
async fn busybox_ash_receives_exactly_the_arguments() {
    let (session, _) = adopt().await;
    let tricky = [
        "a b",
        "it's",
        "$HOME",
        "*",
        "",
        "line\nbreak",
        "; rm -rf /",
        "`id`",
        "\\n",
    ];
    let mut argv = vec!["printf", r"[%s]\n"];
    argv.extend(tricky);
    let expected = tricky.map(|arg| format!("[{arg}]\n")).concat();
    assert_eq!(run(&session, &argv).await, expected);
}

#[tokio::test]
#[ignore = "needs dropbear in an OpenWrt container: just test-dropbear"]
async fn stdin_round_trips_and_streams_are_interactive() {
    let (session, _) = adopt().await;
    let data: Vec<u8> = (0..3_000_000_u32).map(|n| n.to_le_bytes()[0]).collect();
    let output = session
        .exec(&ExecRequest::new(["cat"]).stdin(data.clone()))
        .await
        .unwrap();
    assert_eq!(output.stdout.len(), data.len());
    // Not assert_eq: a mismatch would print megabytes.
    assert!(output.stdout.iter().eq(&data), "the data came back changed");

    let mut stream = session.stream(&["cat"]).await.unwrap();
    stream.write("ping\n").await.unwrap();
    assert_eq!(
        stream.next().await.unwrap().unwrap(),
        Output::Stdout(Bytes::from_static(b"ping\n"))
    );
    stream.close_stdin();
    assert_eq!(
        stream.next().await.unwrap().unwrap(),
        Output::Exit(ExitStatus::Code(0))
    );

    let output = session
        .exec(&ExecRequest::new(["sh", "-c", "echo oops >&2; exit 3"]))
        .await
        .unwrap();
    assert_eq!(
        (output.status, output.stderr),
        (ExitStatus::Code(3), b"oops\n".to_vec())
    );
}

#[tokio::test]
#[ignore = "needs dropbear in an OpenWrt container: just test-dropbear"]
async fn uploads_land_atomically_with_their_mode() {
    let (session, node) = adopt().await;
    let dir = format!(
        "/tmp/uniwrt-test-{}",
        node.key.fingerprint()[7..15].replace('/', "_")
    );
    run(&session, &["mkdir", "-p", &dir]).await;
    let path = format!("{dir}/it's a file");
    session
        .upload(&path, Bytes::from_static(b"first"), 0o600)
        .await
        .unwrap();
    session
        .upload(&path, Bytes::from_static(b"second"), 0o640)
        .await
        .unwrap();
    assert_eq!(run(&session, &["cat", &path]).await, "second");
    assert_eq!(
        run(&session, &["find", &path, "-perm", "640"]).await,
        format!("{path}\n")
    );
    assert_eq!(run(&session, &["ls", &dir]).await, "it's a file\n");
}

#[tokio::test]
#[ignore = "needs dropbear in an OpenWrt container: just test-dropbear"]
async fn a_wrong_pin_is_a_mismatch() {
    let (_, node) = adopt().await;
    let other = HostKey::parse(
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBWCwQCJkKENr7v1OmtQx20yykz6eObDLwYTiz0mnRir",
    )
    .unwrap();
    let credential = Credential::Key(node.key);
    let err = Session::connect(
        &node.target,
        None,
        &other,
        &credential,
        &SshConfig::default(),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(&err, Error::HostKeyMismatch { presented, .. } if *presented == node.host_key),
        "{err:?}"
    );
}

#[tokio::test]
#[ignore = "needs dropbear in an OpenWrt container: just test-dropbear"]
async fn a_jump_through_the_node_reaches_ssh_behind_it() {
    let (session, node) = adopt().await;
    let behind = Target::new("127.0.0.1", 22);
    let config = SshConfig::default();
    assert_eq!(
        probe(&behind, Some(&session), &config).await.unwrap(),
        node.host_key
    );
    let credential = Credential::Key(node.key);
    let tunnelled = Session::connect(
        &behind,
        Some(&session),
        &node.host_key,
        &credential,
        &config,
    )
    .await
    .unwrap();
    assert_eq!(run(&tunnelled, &["echo", "behind"]).await, "behind\n");
}

#[tokio::test]
#[ignore = "needs dropbear in an OpenWrt container: just test-dropbear"]
async fn dropbear_answers_keepalives_so_idle_sessions_stay_up() {
    let (_, node) = adopt().await;
    let config = SshConfig {
        keepalive_interval: Duration::from_secs(1),
        keepalive_max: 2,
        ..SshConfig::default()
    };
    let credential = Credential::Key(node.key);
    let session = Session::connect(&node.target, None, &node.host_key, &credential, &config)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_secs(4)).await;
    assert!(!session.is_closed());
    assert_eq!(run(&session, &["echo", "alive"]).await, "alive\n");
}

#[tokio::test]
#[ignore = "needs dropbear in an OpenWrt container: just test-dropbear"]
async fn a_link_runs_a_ucode_agent() {
    let (_, node) = adopt().await;
    let mut config = LinkConfig::new(node.target, node.host_key, Credential::Key(node.key));
    config.agent = Some(AgentLaunch::Command(vec![
        "ucode".into(),
        "-e".into(),
        ECHO_AGENT.into(),
    ]));
    let link = SshLink::spawn(config, Arc::new(SystemClock));
    let mut state = link.state();
    let up = tokio::time::timeout(LONG, async {
        state
            .wait_for(|state| {
                matches!(
                    state,
                    LinkState::Up {
                        agent: AgentState::Running(_),
                        ..
                    }
                ) || matches!(
                    state,
                    LinkState::Up {
                        agent: AgentState::Failed { .. },
                        ..
                    }
                )
            })
            .await
            .unwrap()
            .clone()
    })
    .await
    .unwrap();
    assert!(
        matches!(
            up,
            LinkState::Up {
                agent: AgentState::Running(_),
                ..
            }
        ),
        "{up:?}"
    );

    let args = to_raw_value(&json!({"sets": ["system"], "n": 1})).unwrap();
    let answer = link.call("echo", &args, LONG).await.unwrap();
    let answer: serde_json::Value = serde_json::from_str(answer.get()).unwrap();
    assert_eq!(answer, json!({"sets": ["system"], "n": 1}));
    let output = link.exec(ExecRequest::new(["echo", "too"])).await.unwrap();
    assert_eq!(output.stdout, b"too\n");
    link.shutdown().await;
}
