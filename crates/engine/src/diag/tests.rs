//! The runs over a fake starter whose streams the tests feed, on tokio's paused clock.

use std::{sync::Arc, time::Duration};

use bytes::Bytes;
use tokio::sync::{broadcast, mpsc};
use uniwrt_core::{ManualClock, NodeId, Timestamp};
use uniwrt_model::diag::{DiagTool, RunState};
use uniwrt_node::{
    Error, ExitStatus, Output,
    exec::{StreamSource, pipe},
};

use super::{DiagConfig, DiagError, DiagUpdate, Diagnostics, Starter, run::Splitter};

const NODE: NodeId = NodeId::new(1);

/// The runs, and each started command's argv and producer's end.
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
            let (stream, source) = pipe("tool", 1 << 20);
            let _ = started.send((argv, source));
            Ok(stream)
        })
    });
    let clock = Arc::new(ManualClock::new(Timestamp::from_unix_ms(1_000)));
    (Diagnostics::new(start, clock, config), starts)
}

fn ping(count: u32) -> DiagTool {
    DiagTool::Ping {
        target: "1.1.1.1".to_owned(),
        family: None,
        count,
    }
}

fn out(text: &'static str) -> Output {
    Output::Stdout(Bytes::from_static(text.as_bytes()))
}

async fn ended(updates: &mut broadcast::Receiver<Arc<DiagUpdate>>) -> Arc<DiagUpdate> {
    loop {
        let update = updates.recv().await.unwrap();
        if update.end.is_some() {
            return update;
        }
    }
}

#[tokio::test(start_paused = true)]
async fn output_streams_as_lines_until_the_tool_exits() {
    let (diag, mut starts) = diagnostics(DiagConfig::default());
    let mut updates = diag.subscribe();
    let run = diag.start(NODE, ping(2), Some("admin".to_owned())).unwrap();
    assert_eq!(run.state, RunState::Running);
    let (_watched, _watch) = diag.watch(run.id).unwrap();
    let (argv, source) = starts.recv().await.unwrap();
    assert_eq!(
        uniwrt_node::diag::tool_argv(&argv),
        ["ping", "-c", "2", "-w", "7", "1.1.1.1"]
    );
    source
        .output
        .push(out("PING 1.1.1.1 (1.1.1.1): 56 data bytes\n64 bytes"));
    let update = updates.recv().await.unwrap();
    assert_eq!(update.lines.len(), 1);
    assert_eq!(
        update.lines[0].text,
        "PING 1.1.1.1 (1.1.1.1): 56 data bytes"
    );
    source.output.push(out(" from 1.1.1.1: seq=0\r\n"));
    source
        .output
        .push(Output::Stderr(Bytes::from_static(b"no reply")));
    source.output.push(Output::Exit(ExitStatus::Code(1)));
    let update = updates.recv().await.unwrap();
    assert_eq!(update.lines[0].text, "64 bytes from 1.1.1.1: seq=0");
    assert_eq!(update.first, 1);
    let update = updates.recv().await.unwrap();
    assert!(
        update.lines[0].err,
        "the unfinished stderr line comes at the end"
    );
    assert_eq!(update.lines[0].text, "no reply");
    let end = ended(&mut updates).await.end.clone().unwrap();
    assert_eq!((end.state, end.exit_code), (RunState::Failed, Some(1)));
    let run = diag.get(run.id).unwrap();
    assert_eq!(run.lines.len(), 3);
    assert_eq!(run.by.as_deref(), Some("admin"));
    assert!(run.ended_at.is_some());
    assert!(diag.list()[0].lines.is_empty(), "lists carry no lines");
}

#[tokio::test(start_paused = true)]
async fn bad_tools_and_too_many_runs_are_refused() {
    let (diag, _starts) = diagnostics(DiagConfig {
        total: 3,
        ..DiagConfig::default()
    });
    let bad = DiagTool::Ping {
        target: "-f".to_owned(),
        family: None,
        count: 1,
    };
    assert!(matches!(
        diag.start(NODE, bad, None),
        Err(DiagError::Invalid(_))
    ));
    diag.start(NODE, ping(5), None).unwrap();
    diag.start(NODE, ping(5), None).unwrap();
    assert!(matches!(
        diag.start(NODE, ping(5), None),
        Err(DiagError::Busy(_))
    ));
    diag.start(NodeId::new(2), ping(5), None).unwrap();
    assert!(matches!(
        diag.start(NodeId::new(3), ping(5), None),
        Err(DiagError::Busy(_))
    ));
    assert_eq!(diag.stop(99), Err(DiagError::NotFound));
}

#[tokio::test(start_paused = true)]
async fn an_admin_stops_a_run_and_the_tool_hears_of_it() {
    let (diag, mut starts) = diagnostics(DiagConfig::default());
    let mut updates = diag.subscribe();
    let run = diag.start(NODE, ping(50), None).unwrap();
    let (_, source) = starts.recv().await.unwrap();
    diag.stop(run.id).unwrap();
    let end = ended(&mut updates).await.end.clone().unwrap();
    assert_eq!((end.state, end.error), (RunState::Stopped, None));
    assert!(!source.output.push(out("late\n")), "the stream is closed");
    // A stopped run frees its place.
    for _ in 0..2 {
        diag.start(NODE, ping(5), None).unwrap();
    }
}

#[tokio::test(start_paused = true)]
async fn a_run_nobody_watches_stops_after_a_minute() {
    let (diag, mut starts) = diagnostics(DiagConfig::default());
    let mut updates = diag.subscribe();
    let run = diag.start(NODE, ping(100), None).unwrap();
    let watch = diag.watch(run.id).unwrap().1;
    let _source = starts.recv().await.unwrap();
    tokio::time::sleep(Duration::from_secs(40)).await;
    assert_eq!(
        diag.get(run.id).unwrap().state,
        RunState::Running,
        "watched"
    );
    drop(watch);
    tokio::time::sleep(Duration::from_secs(50)).await;
    assert_eq!(
        diag.get(run.id).unwrap().state,
        RunState::Running,
        "not yet"
    );
    let end = ended(&mut updates).await.end.clone().unwrap();
    assert_eq!(end.state, RunState::Stopped);
    assert_eq!(
        end.error.as_deref(),
        Some("Stopped: nobody watched it for a minute")
    );
}

#[tokio::test(start_paused = true)]
async fn a_run_stops_at_its_limit() {
    let (diag, mut starts) = diagnostics(DiagConfig::default());
    let mut updates = diag.subscribe();
    let run = diag.start(NODE, ping(1), None).unwrap();
    let _watch = diag.watch(run.id).unwrap().1;
    let _source = starts.recv().await.unwrap();
    let end = ended(&mut updates).await.end.clone().unwrap();
    assert_eq!(end.error.as_deref(), Some("Stopped at its limit of 16 s"));
}

#[tokio::test(start_paused = true)]
async fn output_beyond_the_budget_drops_the_oldest_lines() {
    let (diag, mut starts) = diagnostics(DiagConfig {
        output_budget: 10,
        ..DiagConfig::default()
    });
    let mut updates = diag.subscribe();
    let run = diag.start(NODE, ping(5), None).unwrap();
    let (_, source) = starts.recv().await.unwrap();
    source.output.push(out("aaaa\nbbbb\ncccc\ndddd\n"));
    let update = updates.recv().await.unwrap();
    assert_eq!((update.dropped, update.first), (2, 2));
    let texts: Vec<_> = update.lines.iter().map(|line| line.text.as_str()).collect();
    assert_eq!(texts, ["cccc", "dddd"]);
    assert_eq!(diag.get(run.id).unwrap().dropped, 2);
}

#[tokio::test(start_paused = true)]
async fn a_node_that_cannot_be_reached_fails_the_run() {
    let (diag, _starts) = diagnostics(DiagConfig::default());
    let mut updates = diag.subscribe();
    diag.start(NodeId::new(9), ping(1), None).unwrap();
    let end = ended(&mut updates).await.end.clone().unwrap();
    assert_eq!(end.state, RunState::Failed);
    assert_eq!(
        end.error.as_deref(),
        Some("The device isn't connected: check that it is online")
    );
}

#[tokio::test(start_paused = true)]
async fn ended_runs_are_forgotten_after_a_while() {
    let (diag, _starts) = diagnostics(DiagConfig {
        kept: 1,
        ..DiagConfig::default()
    });
    let mut updates = diag.subscribe();
    let first = diag.start(NodeId::new(9), ping(1), None).unwrap();
    ended(&mut updates).await;
    let second = diag.start(NodeId::new(9), ping(1), None).unwrap();
    ended(&mut updates).await;
    assert!(diag.get(first.id).is_none(), "beyond the count kept");
    assert!(diag.get(second.id).is_some());
    tokio::time::sleep(Duration::from_mins(16)).await;
    assert!(diag.get(second.id).is_none());
}

#[test]
fn long_lines_are_cut() {
    let mut split = Splitter::new(4);
    assert_eq!(split.push(b"abcdefg\nhi"), ["abcd…"]);
    assert_eq!(split.push(b"\xff\n"), ["hi\u{fffd}"]);
    assert_eq!(split.finish(), None);
    assert_eq!(split.push(b"tail"), Vec::<String>::new());
    assert_eq!(split.finish().as_deref(), Some("tail"));
}
