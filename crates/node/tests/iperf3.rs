//! Throughput tests (D-142) with OpenWrt's iperf3 (3.22 on 24.10 and 25.12, installed from the
//! release's feeds when they can be reached): the controller script reads the controller's
//! address from dropbear's environment; a test runs between two sessions, its server waited for
//! until it listens and the client dialing the node's bridge address; and the real client runs each mode against the
//! controller's native server, from a container on the host's network (a host firewall may
//! drop containers' connections to the host; loopback passes). Run by `just test-dropbear`,
//! which names the image in `UNIWRT_IMAGE`.

#![allow(clippy::unwrap_used)]

use std::{net::Ipv4Addr, time::Duration};

use tokio::{net::TcpListener, process::Command};
use uniwrt_model::diag::iperf3::Iperf3Direction;
use uniwrt_node::{
    ExitStatus, Output,
    diag::iperf3::{Far, LISTENING, TO_CONTROLLER, client, server},
    iperf3::{ServerConfig, serve},
};

mod openwrt;

use openwrt::{adopt, drain, install, run};

#[tokio::test]
#[ignore = "needs dropbear in an OpenWrt container: just test-dropbear"]
async fn the_client_is_pointed_at_the_controller() {
    let (session, _) = adopt().await;
    // printf stands in for iperf3.
    let argv = [
        "sh",
        "-c",
        TO_CONTROLLER,
        "sh",
        "printf",
        "%s\\n",
        "-p",
        "5201",
    ];
    let (text, status) = drain(session.stream(&argv).await.unwrap()).await;
    assert_eq!(status, ExitStatus::Code(0), "{text}");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[..3], ["-p", "5201", "-c"], "{text}");
    assert!(lines[3].parse::<std::net::IpAddr>().is_ok(), "{text}");
}

#[tokio::test]
#[ignore = "needs dropbear in an OpenWrt container: just test-dropbear"]
async fn a_test_runs_between_two_nodes() {
    // Both ends on one container, each over a session of its own, the client dialing the
    // container's address on the bridge (the factory container's root password changes under
    // the dropbear tests).
    let (client_session, _) = adopt().await;
    let (server_session, _) = adopt().await;
    if !install(&client_session, "iperf3").await {
        return;
    }
    let address = run(
        &server_session,
        &["sh", "-c", "ip -4 -o addr show dev eth0"],
    )
    .await;
    let host = address
        .split_whitespace()
        .skip_while(|word| *word != "inet")
        .nth(1)
        .and_then(|cidr| cidr.split('/').next())
        .unwrap_or_else(|| panic!("{address}"))
        .to_owned();
    let mut far = server_session.stream(&server()).await.unwrap();
    let mut said = String::new();
    while !said.contains(LISTENING) {
        match tokio::time::timeout(Duration::from_secs(15), far.next()).await {
            Ok(Some(Ok(Output::Stdout(data) | Output::Stderr(data)))) => {
                said.push_str(&String::from_utf8_lossy(&data));
            }
            other => panic!("{other:?} after {said}"),
        }
    }
    let argv = client(Iperf3Direction::Receive, 2, 2, Far::Node { host: &host });
    let (text, status) = drain(client_session.stream(&argv).await.unwrap()).await;
    assert_eq!(status, ExitStatus::Code(0), "{text}");
    assert!(
        text.starts_with(&format!("Connecting to host {host}, port 5201\n")),
        "{text}"
    );
    assert!(text.contains("Reverse mode"), "{text}");
    assert!(text.contains("[SUM]   0.00-1.00   sec"), "{text}");
    assert!(text.ends_with("iperf Done.\n"), "{text}");
    // With -1, the server ends after the test.
    let (_, status) = drain(far).await;
    assert_eq!(status, ExitStatus::Code(0));
}

#[tokio::test]
#[ignore = "needs Docker and an OpenWrt image: just test-dropbear"]
async fn the_real_client_runs_each_mode_against_the_native_server() {
    let image = std::env::var("UNIWRT_IMAGE").expect("UNIWRT_IMAGE=<OpenWrt rootfs image>");
    let config = ServerConfig {
        allowed: vec![Ipv4Addr::LOCALHOST.into()],
        max_seconds: 60,
        max_streams: 8,
        handshake: Duration::from_secs(10),
    };
    let modes = ["", "-R", "--bidir -P 2"];
    let mut ports = Vec::new();
    let mut servers = Vec::new();
    for _ in modes {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        ports.push(listener.local_addr().unwrap().port());
        servers.push(tokio::spawn(serve(listener, config.clone())));
    }
    let tests: Vec<String> = modes
        .iter()
        .zip(&ports)
        .map(|(mode, port)| {
            format!(
                "iperf3 -c 127.0.0.1 -p {port} -t 2 {mode} -f m --connect-timeout 5000 || exit 3"
            )
        })
        .collect();
    let script = format!(
        "mkdir -p /var/lock
if command -v apk >/dev/null; then apk update && apk add iperf3; else opkg update && opkg install iperf3; fi >/dev/null 2>&1 || exit 2
{}",
        tests.join("\n")
    );
    // No init runs, so nothing touches the host's firewall.
    let output = tokio::time::timeout(
        Duration::from_secs(300),
        Command::new("docker")
            .args([
                "run",
                "--rm",
                "--network",
                "host",
                &image,
                "sh",
                "-c",
                &script,
            ])
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    if output.status.code() == Some(2) {
        eprintln!("iperf3 didn't install, so the native server wasn't checked against it");
        return;
    }
    assert!(
        output.status.success(),
        "{text}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(text.matches("iperf Done.").count(), 3, "{text}");
    let mut summaries = Vec::new();
    for server in servers {
        summaries.push(server.await.unwrap().unwrap());
    }
    let (received, sent) = summaries[0].totals();
    assert!(received > 0 && sent == 0, "{:?}", summaries[0]);
    let (received, sent) = summaries[1].totals();
    assert!(received == 0 && sent > 0, "{:?}", summaries[1]);
    let ids: Vec<u32> = summaries[2]
        .streams
        .iter()
        .map(|stream| stream.id)
        .collect();
    assert_eq!(ids, [1, 3, 4, 5]);
    let (received, sent) = summaries[2].totals();
    assert!(received > 0 && sent > 0, "{:?}", summaries[2]);
    // The client's receiver line in the default mode is the server's count.
    let total_line = text
        .lines()
        .find(|line| line.ends_with("receiver"))
        .unwrap_or_else(|| panic!("{text}"));
    assert!(total_line.contains(" Mbits/sec"), "{text}");
}
