//! Terminals against the in-process server: the pseudo-terminal's type and size, keystrokes
//! both ways, resizes, the shell's exit, and the hangup when a terminal is dropped.

// Test code throughout: clippy.toml allows unwrap in tests, but not in their helpers.
#![allow(clippy::unwrap_used)]

mod support;

use std::time::Duration;

use bytes::Bytes;
use support::{LONG, server_with_key};
use tokio::time::{Instant, sleep, timeout};
use uniwrt_node::{Credential, ExitStatus, Output, PtySize, Session, SshConfig, Terminal};

/// The terminal's output until it ends with `suffix`.
async fn until(terminal: &mut Terminal, suffix: &str) -> String {
    let mut shown = String::new();
    while !shown.ends_with(suffix) {
        match timeout(LONG, terminal.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap()
        {
            Output::Stdout(data) => shown.push_str(&String::from_utf8_lossy(&data)),
            other => panic!("{other:?} after {shown:?}"),
        }
    }
    shown
}

#[tokio::test]
async fn a_terminal_has_its_type_and_size_echoes_resizes_and_exits() {
    let (server, key) = server_with_key(1).await;
    let session = Session::connect(
        &server.target(),
        None,
        &server.host_key(),
        &Credential::Key(key),
        &SshConfig::default(),
    )
    .await
    .unwrap();
    let mut terminal = session
        .terminal(PtySize {
            cols: 100,
            rows: 30,
        })
        .await
        .unwrap();
    assert_eq!(
        until(&mut terminal, "$ ").await,
        "pty xterm-256color 100x30\r\n$ "
    );
    terminal.write(Bytes::from_static(b"ls")).await.unwrap();
    assert_eq!(until(&mut terminal, "ls").await, "ls");
    terminal.resize(PtySize {
        cols: 100,
        rows: 30,
    });
    terminal.resize(PtySize {
        cols: 132,
        rows: 43,
    });
    assert_eq!(until(&mut terminal, "\r\n").await, "size 132x43\r\n");
    terminal.write(Bytes::from_static(b"exit\r")).await.unwrap();
    let end = timeout(LONG, terminal.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(end, Output::Exit(ExitStatus::Code(5)));
    assert!(timeout(LONG, terminal.next()).await.unwrap().is_none());

    // Dropping a terminal closes its channel: the shell is hung up.
    let mut terminal = session.terminal(PtySize::DEFAULT).await.unwrap();
    assert_eq!(
        until(&mut terminal, "$ ").await,
        "pty xterm-256color 80x24\r\n$ "
    );
    assert_eq!(server.hangups(), 0);
    drop(terminal);
    let deadline = Instant::now() + LONG;
    while server.hangups() == 0 {
        assert!(Instant::now() < deadline, "the shell wasn't hung up");
        sleep(Duration::from_millis(10)).await;
    }
    assert!(!session.is_closed());
}
