//! Throughput tests: the far node's server first, the controller's own server for the node only,
//! and one test per device.

use std::time::Duration;

use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpStream,
};
use uniwrt_core::NodeId;
use uniwrt_model::diag::{
    DiagTool, RunState,
    iperf3::{Iperf3Direction, Iperf3Peer},
};
use uniwrt_node::{ExitStatus, Output};

use super::{
    DiagConfig, DiagError, RunHosts,
    tests::{diagnostics, ended, out},
};

const NODE: NodeId = NodeId::new(1);
const PEER: NodeId = NodeId::new(2);

fn iperf3(peer: Iperf3Peer) -> DiagTool {
    DiagTool::Iperf3 {
        peer,
        direction: Iperf3Direction::Receive,
        seconds: 2,
        streams: 1,
    }
}

fn hosts() -> RunHosts {
    RunHosts {
        node: Some("127.0.0.1".to_owned()),
        peer: Some("192.168.1.3".to_owned()),
    }
}

#[tokio::test(start_paused = true)]
async fn the_far_node_listens_before_the_client_starts() {
    let (diag, mut starts) = diagnostics(DiagConfig::default());
    let mut updates = diag.subscribe();
    let run = diag
        .start(NODE, iperf3(Iperf3Peer::Node(PEER)), None, hosts())
        .unwrap();
    let (_run, _watch) = diag.watch(run.id).unwrap();
    let (argv, server) = starts.recv().await.unwrap();
    assert_eq!(
        uniwrt_node::diag::tool_argv(&argv),
        [
            "iperf3",
            "-s",
            "-1",
            "-p",
            "5201",
            "-f",
            "m",
            "--forceflush"
        ]
    );
    server.output.push(out(
        "-----------------------------------------------------------\n",
    ));
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        starts.try_recv().is_err(),
        "the client waits for the server"
    );
    server
        .output
        .push(out("Server listening on 5201 (test #1)\n"));
    let (argv, client) = starts.recv().await.unwrap();
    let argv = uniwrt_node::diag::tool_argv(&argv);
    assert_eq!(argv[..2], ["iperf3", "-p"]);
    assert_eq!(argv[argv.len() - 2..], ["-c", "192.168.1.3"]);
    assert!(argv.contains(&"-R".to_owned()));
    server
        .output
        .push(Output::Stderr(bytes::Bytes::from_static(b"iperf3: oops\n")));
    client.output.push(out(
        "[  5]   0.00-1.00   sec   112 MBytes   941 Mbits/sec\n",
    ));
    client.output.push(Output::Exit(ExitStatus::Code(0)));
    let end = ended(&mut updates).await;
    assert_eq!(end.end.as_ref().unwrap().state, RunState::Succeeded);
    let lines: Vec<String> = diag
        .get(run.id)
        .unwrap()
        .lines
        .into_iter()
        .map(|l| l.text)
        .collect();
    assert!(
        lines.contains(&"far end: iperf3: oops".to_owned()),
        "{lines:?}"
    );
    assert!(
        lines.iter().any(|line| line.contains("941 Mbits/sec")),
        "{lines:?}"
    );
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!server.output.push(out("late\n")), "the far end is closed");
}

#[tokio::test(start_paused = true)]
async fn a_far_node_without_iperf3_fails_the_run() {
    let (diag, mut starts) = diagnostics(DiagConfig::default());
    let mut updates = diag.subscribe();
    let run = diag
        .start(NODE, iperf3(Iperf3Peer::Node(PEER)), None, hosts())
        .unwrap();
    let (_run, _watch) = diag.watch(run.id).unwrap();
    let (_, server) = starts.recv().await.unwrap();
    server.output.push(Output::Stderr(bytes::Bytes::from_static(
        b"sh: iperf3: not found\n",
    )));
    server.output.push(Output::Exit(ExitStatus::Code(127)));
    let end = ended(&mut updates).await.end.clone().unwrap();
    assert_eq!(end.state, RunState::Failed);
    assert_eq!(
        end.error.as_deref(),
        Some("The far device lacks iperf3: install the iperf3 package")
    );
    assert!(starts.try_recv().is_err(), "no client started");
    let (diag, mut starts) = diagnostics(DiagConfig::default());
    let mut updates = diag.subscribe();
    diag.start(NODE, iperf3(Iperf3Peer::Node(PEER)), None, hosts())
        .unwrap();
    let (_, server) = starts.recv().await.unwrap();
    server.output.push(out(
        "iperf3: error - unable to start listener for connections: Address in use\n",
    ));
    server.output.push(Output::Exit(ExitStatus::Code(1)));
    let end = ended(&mut updates).await.end.clone().unwrap();
    assert_eq!(
        end.error.as_deref(),
        Some(
            "iperf3 didn't start on the far device: iperf3: error - unable to start listener for \
             connections: Address in use"
        )
    );
}

#[tokio::test]
async fn the_controller_listens_for_the_node_while_the_test_runs() {
    let config = DiagConfig {
        iperf3_listen: Some("127.0.0.1:0".parse().unwrap()),
        ..DiagConfig::default()
    };
    let (diag, mut starts) = diagnostics(config);
    let mut updates = diag.subscribe();
    let run = diag
        .start(NODE, iperf3(Iperf3Peer::Controller), None, hosts())
        .unwrap();
    let (_run, _watch) = diag.watch(run.id).unwrap();
    let (argv, client) = starts.recv().await.unwrap();
    assert_eq!(argv[2], uniwrt_node::diag::iperf3::TO_CONTROLLER);
    let argv = uniwrt_node::diag::tool_argv(&argv);
    let port: u16 = argv[2].parse().unwrap();
    assert_ne!(port, 0);
    // The node (127.0.0.1) reaches the server, which asks for its parameters.
    let mut ctrl = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    ctrl.write_all(&[b'a'; 37]).await.unwrap();
    assert_eq!(ctrl.read_u8().await.unwrap(), 9);
    drop(ctrl);
    // The server sees the client go before the client's exit reaches the controller.
    tokio::time::sleep(Duration::from_millis(100)).await;
    client.output.push(Output::Exit(ExitStatus::Code(1)));
    let end = ended(&mut updates).await.end.clone().unwrap();
    assert_eq!(end.state, RunState::Failed);
    let lines: Vec<String> = diag
        .get(run.id)
        .unwrap()
        .lines
        .into_iter()
        .map(|l| l.text)
        .collect();
    assert!(
        lines.contains(&"controller: the client ended the test".to_owned()),
        "{lines:?}"
    );
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        TcpStream::connect(("127.0.0.1", port)).await.is_err(),
        "the server is gone with the test"
    );
}

#[tokio::test(start_paused = true)]
async fn a_device_takes_part_in_one_test_at_a_time() {
    let config = DiagConfig {
        iperf3_listen: Some("127.0.0.1:0".parse().unwrap()),
        ..DiagConfig::default()
    };
    let (diag, _starts) = diagnostics(config);
    let busy = |result| matches!(result, Err(DiagError::Busy(_)));
    diag.start(NODE, iperf3(Iperf3Peer::Node(PEER)), None, hosts())
        .unwrap();
    // Either end of the running test, as client or peer.
    assert!(busy(diag.start(
        PEER,
        iperf3(Iperf3Peer::Controller),
        None,
        hosts()
    )));
    assert!(busy(diag.start(
        NodeId::new(3),
        iperf3(Iperf3Peer::Node(NODE)),
        None,
        hosts()
    )));
    diag.start(
        NodeId::new(3),
        iperf3(Iperf3Peer::Controller),
        None,
        hosts(),
    )
    .unwrap();
    assert!(busy(diag.start(
        NodeId::new(4),
        iperf3(Iperf3Peer::Controller),
        None,
        hosts()
    )));
    // Other tools still run.
    let ping = DiagTool::Ping {
        target: "1.1.1.1".to_owned(),
        family: None,
        count: 1,
    };
    diag.start(NODE, ping, None, RunHosts::default()).unwrap();
    assert_eq!(
        diag.start(
            NodeId::new(5),
            iperf3(Iperf3Peer::Node(NodeId::new(5))),
            None,
            hosts()
        ),
        Err(DiagError::Invalid(
            "A device can't test its throughput against itself".to_owned()
        ))
    );
    let (off, _) = diagnostics(DiagConfig::default());
    assert_eq!(
        off.start(NODE, iperf3(Iperf3Peer::Controller), None, hosts()),
        Err(DiagError::Invalid(
            "The controller's iperf3 server is off (--iperf3-listen)".to_owned()
        ))
    );
}
