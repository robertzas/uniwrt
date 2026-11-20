use std::process::{Command, Stdio};

use uniwrt_model::diag::capture::CaptureQuery;

use super::{CAPTURE, PcapCut, command};
use crate::diag::tool_argv;

/// A little-endian pcap file's header (microseconds, Ethernet).
fn file_header() -> Vec<u8> {
    let mut header = vec![0xd4, 0xc3, 0xb2, 0xa1, 2, 0, 4, 0];
    header.extend([0; 8]);
    header.extend(262_144_u32.to_le_bytes());
    header.extend(1_u32.to_le_bytes());
    header
}

/// A record of `len` bytes of `fill`.
fn record(len: u32, fill: u8, big_endian: bool) -> Vec<u8> {
    let bytes = |n: u32| {
        if big_endian {
            n.to_be_bytes()
        } else {
            n.to_le_bytes()
        }
    };
    let mut record = Vec::new();
    record.extend(bytes(1_700_000_000));
    record.extend(bytes(0));
    record.extend(bytes(len));
    record.extend(bytes(len));
    record.extend(std::iter::repeat_n(fill, len as usize));
    record
}

#[test]
fn the_capture_runs_tcpdump_with_its_filter_first() {
    let query = CaptureQuery {
        interface: "br-lan".to_owned(),
        filter: Some(" port 53 ".to_owned()),
        snaplen: Some(128),
        ..CaptureQuery::default()
    };
    let argv = command(&query);
    assert_eq!(argv[..5], ["sh", "-c", CAPTURE, "sh", "port 53"]);
    assert_eq!(
        tool_argv(&argv),
        ["tcpdump", "-i", "br-lan", "-s", "128", "-U", "-w", "-"]
    );
    let argv = command(&CaptureQuery::default());
    assert_eq!(argv[4], "");
    assert_eq!(tool_argv(&argv), ["tcpdump", "-i", "any", "-U", "-w", "-"]);
    assert!(CAPTURE.starts_with("exec 3<&0 0</dev/null\nuser=$1; shift\n"));
    assert!(CAPTURE.ends_with("exit \"$status\""));
}

/// The arguments the script hands its tool (`printf`), with `env` set and `filter` given. Its
/// stdin stays open until it exits, as an SSH channel's does: at its end the watchdog would end
/// the tool, perhaps before it printed.
fn handed(env: &[(&str, &str)], filter: &str) -> Vec<String> {
    let mut child = Command::new("sh")
        .args(["-c", CAPTURE, "sh", filter, "printf", "%s\\n"])
        .env_remove("SSH_CONNECTION")
        .env_remove("SSH_CLIENT")
        .envs(env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdin = child.stdin.take();
    let output = child.wait_with_output().unwrap();
    drop(stdin);
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn the_controllers_own_session_is_left_out() {
    let connection = [("SSH_CONNECTION", "172.17.0.1 51234 172.17.0.3 22")];
    assert_eq!(
        handed(&connection, "icmp or (udp and port 53)"),
        [
            "--",
            "(icmp or (udp and port 53)) and not (tcp and host 172.17.0.1 and port 51234 and port 22)"
        ]
    );
    assert_eq!(
        handed(&[("SSH_CLIENT", "fe80::1%br-lan 40000 2222")], ""),
        [
            "--",
            "not (tcp and host fe80::1 and port 40000 and port 2222)"
        ]
    );
    assert_eq!(
        handed(&[("SSH_CLIENT", "::ffff:10.0.0.5 40000 22")], "arp"),
        [
            "--",
            "(arp) and not (tcp and host 10.0.0.5 and port 40000 and port 22)"
        ]
    );
    assert_eq!(handed(&[], "arp"), ["--", "arp"]);
    // No filter at all: printf prints its format once, with no argument.
    assert_eq!(handed(&[], ""), [""]);
}

#[test]
fn a_capture_passes_whole_packets_up_to_its_cap() {
    let mut stream = file_header();
    for fill in 1..=5 {
        stream.extend(record(100, fill, false));
    }
    // The header, and three packets of 116 bytes: a fourth would pass 400.
    let max = 24 + 3 * 116 + 50;
    for chunk in [1, 7, 16, 23, 116, 4096] {
        let mut cut = PcapCut::new(max);
        let mut out = Vec::new();
        for part in stream.chunks(chunk) {
            out.extend(cut.push(part));
        }
        assert_eq!(out, stream[..24 + 3 * 116], "chunks of {chunk}");
        assert!(cut.done());
        assert_eq!((cut.bytes(), cut.packets()), (24 + 3 * 116, 3));
    }
    let mut cut = PcapCut::new(1 << 20);
    assert_eq!(cut.push(&stream), stream);
    assert!(!cut.done());
    assert_eq!(cut.packets(), 5);
}

#[test]
fn big_endian_and_unknown_streams_are_cut_too() {
    let mut stream = vec![0xa1, 0xb2, 0xc3, 0xd4];
    stream.extend([0; 20]);
    stream.extend(record(0, 0, true));
    stream.extend(record(40, 9, true));
    stream.extend(record(40, 9, true));
    let mut cut = PcapCut::new(24 + 16 + 56 + 10);
    assert_eq!(cut.push(&stream), stream[..24 + 16 + 56]);
    assert_eq!(cut.packets(), 2);
    // A record longer than any snapshot length ends it.
    let mut stream = file_header();
    stream.extend(record(300_000, 1, false));
    let mut cut = PcapCut::new(1 << 20);
    assert_eq!(cut.push(&stream), file_header());
    assert!(cut.done());
    // Not a pcap file: bytes up to the cap.
    let mut cut = PcapCut::new(30);
    assert_eq!(cut.push(&[7; 20]), [7; 0]);
    assert_eq!(cut.push(&[7; 20]), [7; 30]);
    assert!(cut.done());
}
