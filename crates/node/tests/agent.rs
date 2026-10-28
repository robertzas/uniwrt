//! The node agent on real OpenWrt (TESTING.md, Agent): installing the bundle, the ops in both
//! loop modes, and collect sets parsed into facts. Ignored by default: `just test-agent` boots
//! an OpenWrt rootfs container per release with procd, so ubus, netifd and logd run, and runs
//! these against it with `UNIWRT_NODE=<host:port>`, `UNIWRT_NODE_PASSWORD` and
//! `UNIWRT_NODE_RELEASE`.

// Test code throughout: clippy.toml allows unwrap in tests, but not in their helpers.
#![allow(clippy::unwrap_used)]

mod openwrt;

use std::{
    env,
    time::{Duration, Instant},
};

use openwrt::{adopt, link, run};
use uniwrt_core::{Clock as _, SystemClock};
use uniwrt_model::facts::{DeviceKind, NodeFacts, PackageManager};
use uniwrt_node::{
    AgentLaunch, Bundle, Error, NodeLink as _, Output, facts,
    proto::ops::{Collect, Exec, Ping, ReadFile, Set, Ubus, code},
};

const LONG: Duration = Duration::from_secs(20);

fn release() -> String {
    env::var("UNIWRT_NODE_RELEASE").expect("UNIWRT_NODE_RELEASE")
}

/// The 25.12 rootfs has ucode-mod-uloop and the 24.10 one has not (24.10 images with Wi-Fi
/// have it, through hostapd), so 24.10 tests the synchronous mode.
fn has_event_loop() -> bool {
    !release().starts_with("24.")
}

fn remote_code<T: std::fmt::Debug>(result: Result<T, Error>) -> String {
    match result {
        Err(Error::Remote { code, .. }) => code,
        other => panic!("expected an agent error, got {other:?}"),
    }
}

#[tokio::test]
#[ignore = "needs an OpenWrt container booted with procd: just test-agent"]
async fn the_bundle_installs_by_hash_and_its_agent_says_hello() {
    let (session, node) = adopt().await;
    let bundle = Bundle::get();
    bundle.ensure(&session).await.unwrap();
    assert!(!bundle.ensure(&session).await.unwrap(), "installed already");
    let version = run(&session, &["cat", "/tmp/uniwrt/agent/VERSION"]).await;
    assert_eq!(version.trim(), bundle.hash());
    let mode = run(&session, &["ls", "-ld", "/tmp/uniwrt"]).await;
    assert!(mode.starts_with("drwx------"), "{mode}");

    // Another version on the node is replaced.
    run(
        &session,
        &["sh", "-c", "echo stale >/tmp/uniwrt/agent/VERSION"],
    )
    .await;
    assert!(bundle.ensure(&session).await.unwrap());

    let (link, hello) = link(&node, AgentLaunch::Bundle).await;
    assert_eq!(hello.agent, bundle.hash());
    assert_eq!(hello.release.as_deref(), Some(release().as_str()));
    assert_eq!(hello.caps.events, has_event_loop());
    assert!(Set::ALL.iter().all(|&set| hello.caps.has_set(set)));
    assert_eq!(
        hello.caps.ops,
        ["collect", "exec", "ping", "read_file", "ubus"]
    );
    link.shutdown().await;
}

#[tokio::test]
#[ignore = "needs an OpenWrt container booted with procd: just test-agent"]
async fn collected_sets_parse_into_facts() {
    let (_, node) = adopt().await;
    let (link, _) = link(&node, AgentLaunch::Bundle).await;
    let mut facts = NodeFacts::default();
    for _ in 0..2 {
        // The second round has a CPU percentage: the first only takes the baseline.
        let collected = link.request(&Collect::all(), LONG).await.unwrap();
        assert_eq!(facts::apply(&collected, &mut facts), []);
        tokio::time::sleep(Duration::from_millis(600)).await;
    }
    assert_eq!(facts.board.release.version, release());
    assert_eq!(facts.board.release.arch, "x86_64");
    assert!(!facts.board.hostname.is_empty(), "{:?}", facts.board);
    assert!(facts.system.uptime_s > 0 && facts.system.memory.total_bytes > 0);
    assert!(facts.system.cpu_percent.is_some());
    let lan = facts.interfaces.iter().find(|i| i.name == "lan").unwrap();
    assert!(lan.up, "{lan:?}");
    assert_eq!(lan.device.as_deref(), Some("br-lan"));
    assert_eq!(lan.ipv4.len(), 1, "the container's address");
    let bridge = facts.devices.iter().find(|d| d.name == "br-lan").unwrap();
    assert_eq!(bridge.kind, DeviceKind::Bridge);
    let port = facts.devices.iter().find(|d| d.name == "eth0").unwrap();
    assert_eq!(port.master.as_deref(), Some("br-lan"));
    assert!(port.counters.rx_bytes > 0);
    let expected = if release().starts_with("24.") {
        PackageManager::Opkg
    } else {
        PackageManager::Apk
    };
    assert_eq!(facts.capabilities.package_manager, Some(expected));
    link.shutdown().await;
}

#[tokio::test]
#[ignore = "needs an OpenWrt container booted with procd: just test-agent"]
async fn ops_answer_or_fail_with_a_code() {
    let (_, node) = adopt().await;
    let (link, _) = link(&node, AgentLaunch::Bundle).await;

    let pong = link.request(&Ping {}, LONG).await.unwrap();
    let skew = pong.ts.as_unix_ms() - SystemClock.now().as_unix_ms();
    assert!(
        skew.abs() < 60_000,
        "the container shares the host's clock: {skew} ms"
    );

    let board = link
        .request(&Ubus::new("system", "board"), LONG)
        .await
        .unwrap();
    assert!(board.get().contains(r#""board_name""#));
    let missing = link.request(&Ubus::new("system", "nope"), LONG).await;
    assert_eq!(remote_code(missing), code::NOT_FOUND);

    let tricky = ["printf", "%s|", "a b", "it's", "$HOME", "*", "line\nbreak"];
    let printed = link.request(&Exec::new(tricky), LONG).await.unwrap();
    assert_eq!(printed.stdout, "a b|it's|$HOME|*|line\nbreak|");
    let failed = link
        .request(&Exec::new(["sh", "-c", "echo oops >&2; exit 3"]), LONG)
        .await
        .unwrap();
    assert_eq!((failed.code, failed.stderr.as_str()), (Some(3), "oops\n"));
    let killed = link
        .request(&Exec::new(["sh", "-c", "kill -TERM $$"]), LONG)
        .await
        .unwrap();
    assert_eq!((killed.code, killed.signal), (None, Some(15)));
    let mut slow = Exec::new(["sleep", "5"]);
    slow.timeout_ms = 300;
    assert_eq!(remote_code(link.request(&slow, LONG).await), code::TIMEOUT);
    let mut chatty = Exec::new(["head", "-c", "5000", "/dev/zero"]);
    chatty.max_output = 1000;
    assert_eq!(
        remote_code(link.request(&chatty, LONG).await),
        code::TOO_LARGE
    );
    let missing = link
        .request(&Exec::new(["no-such-command"]), LONG)
        .await
        .unwrap();
    assert_eq!(missing.code, Some(127));

    let mut head = ReadFile::new("/etc/openwrt_release");
    head.max_bytes = 10;
    let file = link.request(&head, LONG).await.unwrap();
    assert_eq!((file.data.as_str(), file.truncated), ("DISTRIB_ID", true));
    let mut binary = ReadFile::new("/etc/openwrt_release");
    binary.max_bytes = 3;
    binary.base64 = true;
    assert_eq!(link.request(&binary, LONG).await.unwrap().data, "RElT");
    let missing = link.request(&ReadFile::new("/nope"), LONG).await;
    assert_eq!(remote_code(missing), code::NOT_FOUND);

    let args = serde_json::value::to_raw_value(&serde_json::json!({})).unwrap();
    let unknown = link.call("bogus", &args, LONG).await;
    assert_eq!(remote_code(unknown), code::BAD_REQUEST);
    link.shutdown().await;
}

#[tokio::test]
#[ignore = "needs an OpenWrt container booted with procd: just test-agent"]
async fn the_synchronous_mode_serves_the_same_ops() {
    let (session, node) = adopt().await;
    let bundle = Bundle::get();
    bundle.ensure(&session).await.unwrap();
    let mut argv = bundle.argv();
    argv.push("--sync".to_owned());
    let (link, hello) = link(&node, AgentLaunch::Command(argv)).await;
    assert!(!hello.caps.events);
    let collected = link
        .request(
            &Collect {
                sets: vec![Set::System, Set::Board],
            },
            LONG,
        )
        .await
        .unwrap();
    let mut facts = NodeFacts::default();
    assert_eq!(facts::apply(&collected, &mut facts), []);
    assert_eq!(facts.board.release.version, release());
    let mut slow = Exec::new(["sleep", "5"]);
    slow.timeout_ms = 300;
    assert_eq!(remote_code(link.request(&slow, LONG).await), code::TIMEOUT);
    let echoed = link.request(&Exec::new(["echo", "hi"]), LONG).await;
    assert_eq!(echoed.unwrap().stdout, "hi\n");
    link.shutdown().await;
}

#[tokio::test]
#[ignore = "needs an OpenWrt container booted with procd: just test-agent"]
async fn under_the_event_loop_a_slow_command_holds_up_nothing() {
    if !has_event_loop() {
        return;
    }
    let (_, node) = adopt().await;
    let (link, _) = link(&node, AgentLaunch::Bundle).await;
    let mut slow = Exec::new(["sleep", "2"]);
    slow.timeout_ms = 10_000;
    let started = Instant::now();
    let (slept, pinged) = tokio::join!(link.request(&slow, LONG), async {
        tokio::time::sleep(Duration::from_millis(200)).await;
        link.request(&Ping {}, LONG).await.unwrap();
        started.elapsed()
    });
    assert!(slept.unwrap().success());
    assert!(pinged < Duration::from_secs(1), "{pinged:?}");
    link.shutdown().await;
}

/// Closing the agent's stdin ends it, even with a command still running.
#[tokio::test]
#[ignore = "needs an OpenWrt container booted with procd: just test-agent"]
async fn the_agent_exits_when_its_stdin_closes() {
    let (session, _) = adopt().await;
    let bundle = Bundle::get();
    bundle.ensure(&session).await.unwrap();
    let mut stream = session.stream(&bundle.argv()).await.unwrap();
    stream
        .write(
            r#"{"id":1,"op":"exec","args":{"argv":["sleep","3"],"timeout_ms":10000}}"#.to_owned()
                + "\n",
        )
        .await
        .unwrap();
    let mut stdout = String::new();
    while !stdout.contains("hello") {
        match stream.next().await.unwrap().unwrap() {
            Output::Stdout(data) => stdout.push_str(std::str::from_utf8(&data).unwrap()),
            other => panic!("{other:?}"),
        }
    }
    // The synchronous agent is in the middle of the command, and reads EOF after it.
    let limit = if has_event_loop() { 2 } else { 5 };
    let closed = Instant::now();
    stream.close_stdin();
    let exit = tokio::time::timeout(Duration::from_secs(limit), async {
        loop {
            if let Output::Exit(status) = stream.next().await.unwrap().unwrap() {
                return status;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("still running after {:?}", closed.elapsed()));
    assert!(exit.success(), "{exit:?}");
}
