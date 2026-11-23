//! Terminals against dropbear on real OpenWrt (D-141): root's ash on a pseudo-terminal, `$TERM`,
//! a resize that reaches the foreground job as `SIGWINCH` (OpenWrt has no `stty` to print the
//! size), its exit status, and the hangup that ends the shell when the terminal is dropped. Ignored by default: `just test-dropbear` runs it against
//! an OpenWrt rootfs container per release.

// Test code throughout: clippy.toml allows unwrap in tests, but not in their helpers.
#![allow(clippy::unwrap_used)]

use std::time::Duration;

use bytes::Bytes;
use tokio::time::{Instant, sleep, timeout};
use uniwrt_node::{ExitStatus, Output, PtySize, Terminal};

mod openwrt;

use openwrt::{adopt, run};

const LONG: Duration = Duration::from_secs(10);

/// The terminal's output until it ends with ash's root prompt.
async fn prompt(terminal: &mut Terminal) -> String {
    let mut shown = String::new();
    while !shown.ends_with("# ") {
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

/// Types `line` and answers what the shell printed after it, up to the next prompt.
async fn typed(terminal: &mut Terminal, line: &str) -> String {
    terminal
        .write(Bytes::from(format!("{line}\r")))
        .await
        .unwrap();
    let shown = prompt(terminal).await;
    let echoed = format!("{line}\r\n");
    shown
        .split_once(&echoed)
        .map_or(shown.clone(), |(_, after)| after.to_owned())
}

#[tokio::test]
#[ignore = "needs dropbear in an OpenWrt container: just test-dropbear"]
async fn a_terminal_runs_ash_on_a_pty_that_resizes_exits_and_hangs_up() {
    let (session, _) = adopt().await;
    let mut terminal = session
        .terminal(PtySize {
            cols: 100,
            rows: 30,
        })
        .await
        .unwrap();
    prompt(&mut terminal).await;
    assert!(
        typed(&mut terminal, "echo $TERM")
            .await
            .starts_with("xterm-256color\r\n")
    );
    // The foreground job hears the resize (signal 28, SIGWINCH) well before its sleep ends.
    let job = r#"sh -c 'trap "echo winch" 28; echo waiting; sleep 8 & wait'"#;
    terminal
        .write(Bytes::from(format!("{job}\r")))
        .await
        .unwrap();
    let mut shown = String::new();
    while !shown.contains("waiting\r\n") {
        let Output::Stdout(data) = timeout(LONG, terminal.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap()
        else {
            panic!("the job ended early: {shown:?}")
        };
        shown.push_str(&String::from_utf8_lossy(&data));
    }
    let resized = Instant::now();
    terminal.resize(PtySize {
        cols: 132,
        rows: 43,
    });
    let after = prompt(&mut terminal).await;
    assert!(after.contains("winch\r\n"), "{after:?}");
    assert!(
        resized.elapsed() < Duration::from_secs(4),
        "the job ran its course"
    );
    terminal
        .write(Bytes::from_static(b"exit 3\r"))
        .await
        .unwrap();
    let end = loop {
        match timeout(LONG, terminal.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap()
        {
            Output::Exit(status) => break status,
            Output::Stdout(_) | Output::Stderr(_) => {}
        }
    };
    assert_eq!(end, ExitStatus::Code(3));

    // A dropped terminal's shell is hung up, and whatever runs in it.
    let mut terminal = session.terminal(PtySize::DEFAULT).await.unwrap();
    prompt(&mut terminal).await;
    let pid = typed(&mut terminal, "echo $$").await;
    let pid = pid.lines().next().unwrap().trim().to_owned();
    assert!(pid.parse::<u32>().is_ok(), "{pid:?}");
    drop(terminal);
    let deadline = Instant::now() + LONG;
    loop {
        let alive = run(
            &session,
            &[
                "sh",
                "-c",
                "kill -0 \"$1\" 2>/dev/null && echo alive || echo gone",
                "sh",
                &pid,
            ],
        )
        .await;
        if alive.trim() == "gone" {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the shell {pid} outlived its terminal"
        );
        sleep(Duration::from_millis(100)).await;
    }
}
