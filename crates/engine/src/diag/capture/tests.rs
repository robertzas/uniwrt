//! Captures over a fake starter whose streams the tests feed, on tokio's paused clock.

use std::{sync::Arc, time::Duration};

use bytes::Bytes;
use tokio::sync::mpsc;
use uniwrt_core::{ManualClock, NodeId, Timestamp};
use uniwrt_model::diag::capture::CaptureQuery;
use uniwrt_node::{
    Error, ExitStatus, Output,
    exec::{StreamSource, pipe},
};

use super::{CaptureEnd, CaptureError};
use crate::diag::{DiagConfig, Diagnostics, Starter};

const NODE: NodeId = NodeId::new(1);

fn diagnostics(
    config: DiagConfig,
) -> (
    Diagnostics,
    mpsc::UnboundedReceiver<(Vec<String>, StreamSource)>,
) {
    let (started, starts) = mpsc::unbounded_channel();
    let start: Starter = Arc::new(move |node: NodeId, argv: Vec<String>| {
        let started = started.clone();
        Box::pin(async move {
            if node == NodeId::new(9) {
                return Err(Error::Disconnected {
                    what: argv[0].clone(),
                });
            }
            let (stream, source) = pipe("tcpdump", 8 << 20);
            let _ = started.send((argv, source));
            Ok(stream)
        })
    });
    let clock = Arc::new(ManualClock::new(Timestamp::from_unix_ms(1_000)));
    (Diagnostics::new(start, clock, config), starts)
}

fn header() -> Vec<u8> {
    let mut header = vec![0xd4, 0xc3, 0xb2, 0xa1, 2, 0, 4, 0];
    header.extend([0; 8]);
    header.extend(262_144_u32.to_le_bytes());
    header.extend(1_u32.to_le_bytes());
    header
}

fn packet(len: u32) -> Vec<u8> {
    let mut record = vec![0; 8];
    record.extend(len.to_le_bytes());
    record.extend(len.to_le_bytes());
    record.extend(std::iter::repeat_n(0xab, len as usize));
    record
}

fn stdout(data: Vec<u8>) -> Output {
    Output::Stdout(Bytes::from(data))
}

#[tokio::test(start_paused = true)]
async fn a_capture_streams_whole_packets_to_its_size_cap() {
    let (diag, mut starts) = diagnostics(DiagConfig::default());
    let query = CaptureQuery {
        interface: "br-lan".to_owned(),
        filter: Some("port 53".to_owned()),
        max_mib: 1,
        ..CaptureQuery::default()
    };
    let capturing = tokio::spawn({
        let diag = diag.clone();
        async move { diag.capture(NODE, &query).await }
    });
    let (argv, source) = starts.recv().await.unwrap();
    assert_eq!(argv[4], "port 53");
    assert_eq!(argv[5], "tcpdump");
    source.output.push(Output::Stderr(Bytes::from_static(
        b"tcpdump: listening on br-lan, link-type EN10MB (Ethernet), snapshot length 262144 bytes\n",
    )));
    source.output.push(stdout(header()));
    let mut capture = capturing.await.unwrap().unwrap();
    assert_eq!(capture.next().await.unwrap(), header());
    // One capture per node at a time.
    assert!(matches!(
        diag.capture(NODE, &CaptureQuery::default()).await,
        Err(CaptureError::Busy(_))
    ));
    // 200 KiB packets: a sixth would pass 1 MiB.
    for _ in 0..6 {
        source.output.push(stdout(packet(200 << 10)));
    }
    let mut passed = 0;
    while let Some(bytes) = capture.next().await {
        assert_eq!(bytes.len(), 16 + (200 << 10));
        passed += 1;
    }
    assert_eq!((passed, capture.packets()), (5, 5));
    assert_eq!(capture.end(), Some(&CaptureEnd::Size));
    assert_eq!(capture.bytes(), 24 + 5 * (16 + (200 << 10)));
    drop(capture);
    let again = tokio::spawn({
        let diag = diag.clone();
        async move { diag.capture(NODE, &CaptureQuery::default()).await }
    });
    let (_, source) = starts.recv().await.unwrap();
    source.output.push(stdout(header()));
    assert!(again.await.unwrap().is_ok(), "the slot is free again");
}

#[tokio::test(start_paused = true)]
async fn a_capture_ends_at_its_time_cap_and_with_tcpdump() {
    let (diag, mut starts) = diagnostics(DiagConfig {
        captures: 1,
        ..DiagConfig::default()
    });
    let query = CaptureQuery {
        seconds: 30,
        ..CaptureQuery::default()
    };
    let capturing = tokio::spawn({
        let diag = diag.clone();
        async move { diag.capture(NODE, &query).await }
    });
    let (_, source) = starts.recv().await.unwrap();
    source.output.push(stdout(header()));
    let mut capture = capturing.await.unwrap().unwrap();
    assert!(matches!(
        diag.capture(NodeId::new(2), &CaptureQuery::default()).await,
        Err(CaptureError::Busy(why)) if why.starts_with("1 captures")
    ));
    capture.next().await.unwrap();
    let start = tokio::time::Instant::now();
    assert_eq!(capture.next().await, None);
    assert_eq!(capture.end(), Some(&CaptureEnd::Time));
    assert!(start.elapsed() >= Duration::from_secs(29));
    drop(capture);

    let capturing = tokio::spawn({
        let diag = diag.clone();
        async move { diag.capture(NODE, &CaptureQuery::default()).await }
    });
    let (_, source) = starts.recv().await.unwrap();
    source.output.push(stdout(header()));
    source
        .output
        .push(Output::Exit(ExitStatus::Signal("TERM".to_owned())));
    let mut capture = capturing.await.unwrap().unwrap();
    capture.next().await.unwrap();
    assert_eq!(capture.next().await, None);
    assert_eq!(
        capture.end(),
        Some(&CaptureEnd::Exited(ExitStatus::Signal("TERM".to_owned())))
    );
}

/// What a capture whose tcpdump prints `stderr` and exits with `code` answers.
async fn refused(stderr: &'static str, code: u32) -> CaptureError {
    let (diag, mut starts) = diagnostics(DiagConfig::default());
    let capturing = tokio::spawn(async move { diag.capture(NODE, &CaptureQuery::default()).await });
    let (_, source) = starts.recv().await.unwrap();
    source
        .output
        .push(Output::Stderr(Bytes::from_static(stderr.as_bytes())));
    source.output.push(Output::Exit(ExitStatus::Code(code)));
    capturing.await.unwrap().unwrap_err()
}

#[tokio::test(start_paused = true)]
async fn refusals_are_worded_before_the_download_starts() {
    assert_eq!(
        refused(
            "tcpdump: WARNING: any: That device doesn't support promiscuous mode\n(Promiscuous mode not supported on the \"any\" device)\ntcpdump: unknown port 'nope'\n",
            1
        )
        .await,
        CaptureError::Refused("unknown port 'nope'".to_owned())
    );
    assert_eq!(
        refused(
            "tcpdump: lan9: No such device exists\n(No such device exists)\n",
            1
        )
        .await
        .to_string(),
        "tcpdump refused it: lan9: No such device exists"
    );
    assert_eq!(
        refused("sh: tcpdump: not found\n", 127).await,
        CaptureError::Missing("tcpdump-mini")
    );
    assert_eq!(
        refused("", 2).await,
        CaptureError::Refused("it exited with 2".to_owned())
    );
    let (diag, _starts) = diagnostics(DiagConfig::default());
    assert_eq!(
        diag.capture(NodeId::new(9), &CaptureQuery::default())
            .await
            .unwrap_err(),
        CaptureError::Offline
    );
    let bad = CaptureQuery {
        filter: Some("(tcp".to_owned()),
        ..CaptureQuery::default()
    };
    assert!(matches!(
        diag.capture(NODE, &bad).await,
        Err(CaptureError::Invalid(_))
    ));
}

#[tokio::test(start_paused = true)]
async fn a_tcpdump_that_never_starts_times_out() {
    let (diag, mut starts) = diagnostics(DiagConfig::default());
    let capturing = tokio::spawn(async move { diag.capture(NODE, &CaptureQuery::default()).await });
    let (_, _source) = starts.recv().await.unwrap();
    assert_eq!(
        capturing.await.unwrap().unwrap_err(),
        CaptureError::Failed("tcpdump didn't start within 15 s".to_owned())
    );
}
