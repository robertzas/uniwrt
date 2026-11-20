//! Packet captures (D-142) over dropbear on OpenWrt rootfs containers: the capture script reads
//! the controller's session from dropbear's environment, and a real tcpdump (`tcpdump-mini`,
//! installed from the release's feeds when they can be reached) captures without it, ending when
//! its stream is dropped. Run by `just test-dropbear`.

#![allow(clippy::unwrap_used)]

use std::time::Duration;

use uniwrt_model::diag::capture::CaptureQuery;
use uniwrt_node::{
    ExecRequest, ExitStatus, Output,
    diag::capture::{CAPTURE, command},
};

mod openwrt;

use openwrt::{adopt, drain, run};

#[tokio::test]
#[ignore = "needs dropbear in an OpenWrt container: just test-dropbear"]
async fn the_capture_script_leaves_the_controllers_session_out() {
    let (session, _) = adopt().await;
    // dropbear names the session in the environment; printf stands in for tcpdump.
    let argv = ["sh", "-c", CAPTURE, "sh", "icmp", "printf", "%s\\n"];
    let (text, status) = drain(session.stream(&argv).await.unwrap()).await;
    assert_eq!(status, ExitStatus::Code(0), "{text}");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2, "{text}");
    assert_eq!(lines[0], "--");
    let own = lines[1]
        .strip_prefix("(icmp) and not (tcp and host ")
        .and_then(|rest| rest.strip_suffix(')'))
        .unwrap_or_else(|| panic!("{text}"));
    let words: Vec<&str> = own.split(' ').collect();
    assert!(
        matches!(words[..], [host, "and", "port", from, "and", "port", to]
            if host.parse::<std::net::IpAddr>().is_ok()
                && from.parse::<u16>().is_ok()
                && to.parse::<u16>().is_ok()),
        "{text}"
    );
}

/// Installs tcpdump-mini; false when the feeds can't be reached.
async fn install_tcpdump(session: &uniwrt_node::Session) -> bool {
    let apk = run(session, &["sh", "-c", "command -v apk || true"]).await;
    let script = if apk.trim().is_empty() {
        "mkdir -p /var/lock && opkg update && opkg install tcpdump-mini"
    } else {
        "mkdir -p /var/lock && apk update && apk add tcpdump-mini"
    };
    let request = ExecRequest::new(["sh", "-c", script]).timeout(Duration::from_secs(240));
    match session.exec(&request).await {
        Ok(output) if output.status.success() => true,
        other => {
            let why = other.map_or_else(
                |err| err.to_string(),
                |output| String::from_utf8_lossy(&output.stderr).into_owned(),
            );
            eprintln!("tcpdump-mini didn't install, so no capture was checked: {why}");
            false
        }
    }
}

/// The TCP ports of each packet of a Linux cooked v2 capture, and how many were ICMP.
fn tcp_ports_and_icmp(pcap: &[u8]) -> (Vec<(u16, u16)>, usize) {
    assert_eq!(pcap[..4], [0xd4, 0xc3, 0xb2, 0xa1]);
    assert_eq!(pcap[20..24], 276_u32.to_le_bytes(), "LINUX_SLL2");
    let (mut ports, mut icmp) = (Vec::new(), 0);
    let mut at = 24;
    while at + 16 <= pcap.len() {
        let len = u32::from_le_bytes(pcap[at + 8..at + 12].try_into().unwrap()) as usize;
        let Some(frame) = pcap.get(at + 16..at + 16 + len) else {
            break;
        };
        at += 16 + len;
        let ip = &frame[20..];
        let (proto, l4) = match u16::from_be_bytes([frame[0], frame[1]]) {
            0x0800 => (ip[9], &ip[usize::from(ip[0] & 0xf) * 4..]),
            0x86dd => (ip[6], &ip[40..]),
            _ => continue,
        };
        match proto {
            6 => ports.push((
                u16::from_be_bytes([l4[0], l4[1]]),
                u16::from_be_bytes([l4[2], l4[3]]),
            )),
            1 | 58 => icmp += 1,
            _ => {}
        }
    }
    (ports, icmp)
}

#[tokio::test]
#[ignore = "needs dropbear in an OpenWrt container: just test-dropbear"]
async fn tcpdump_captures_without_the_controllers_session_and_ends_with_its_stream() {
    let (session, _) = adopt().await;
    if !install_tcpdump(&session).await {
        return;
    }
    let connection = run(&session, &["sh", "-c", "echo $SSH_CONNECTION"]).await;
    let words: Vec<u16> = connection
        .split_whitespace()
        .skip(1)
        .step_by(2)
        .map(|port| port.parse().unwrap())
        .collect();
    let own = (words[0], words[1]);
    let mut stream = session
        .stream(&command(&CaptureQuery::default()))
        .await
        .unwrap();
    let mut pcap = Vec::new();
    let read = async {
        let until = tokio::time::Instant::now() + Duration::from_secs(4);
        while let Ok(Some(next)) = tokio::time::timeout_at(until, stream.next()).await {
            match next.unwrap() {
                Output::Stdout(data) => pcap.extend_from_slice(&data),
                Output::Stderr(_) => {}
                Output::Exit(status) => panic!("tcpdump exited: {status:?}"),
            }
        }
    };
    let ping = async {
        tokio::time::sleep(Duration::from_millis(500)).await;
        run(&session, &["ping", "-c", "2", "127.0.0.1"]).await
    };
    let ((), _) = tokio::join!(read, ping);
    drop(stream);
    let (ports, icmp) = tcp_ports_and_icmp(&pcap);
    assert!(icmp >= 4, "the pings, both ways: {icmp}");
    assert!(
        !ports.iter().any(|&(a, b)| (a, b) == own || (b, a) == own),
        "the controller's own session was captured: {own:?} in {ports:?}"
    );
    let mut left = String::new();
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        left = run(&session, &["sh", "-c", "ps w | grep '[t]cpdump' || true"]).await;
        if left.is_empty() {
            break;
        }
    }
    assert_eq!(left, "", "tcpdump outlived its stream");
}
