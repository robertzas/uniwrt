//! The capture's download on the demo, whose simulated nodes run tcpdump once it is installed.

use axum::http::{StatusCode, header};
use uniwrt_engine::Fleet as _;
use uniwrt_node::ExecRequest;
use uniwrt_store::AuditRepo as _;

use crate::testing::{Demo, json};

/// Installs tcpdump-mini on the gateway, through whichever package manager it has.
async fn install_tcpdump(demo: &Demo) {
    for argv in [["opkg", "install"], ["apk", "add"]] {
        let request = ExecRequest::new(argv.into_iter().chain(["tcpdump-mini"]));
        if let Ok(output) = demo.engine.exec(demo.ids[0], request).await
            && output.status.success()
        {
            return;
        }
    }
    panic!("tcpdump-mini didn't install");
}

#[tokio::test]
async fn a_capture_downloads_as_a_pcap_until_its_time_cap() {
    let demo = Demo::start().await;
    let uri = format!(
        "/v1/nodes/{}/diag/capture?interface=br-lan&filter=port%2053&seconds=1&max_mib=1",
        demo.ids[0]
    );
    let res = demo.send(demo.get(&uri)).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(
        json(res).await["detail"],
        "The device lacks tcpdump: install the tcpdump-mini package"
    );
    install_tcpdump(&demo).await;
    let res = demo.send(demo.get(&uri)).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers()[header::CONTENT_TYPE],
        "application/vnd.tcpdump.pcap"
    );
    let disposition = res.headers()[header::CONTENT_DISPOSITION].to_str().unwrap();
    assert!(
        disposition.starts_with("attachment; filename=\"") && disposition.contains("-br-lan-"),
        "{disposition}"
    );
    assert!(disposition.ends_with(".pcap\""), "{disposition}");
    let audited = demo.store.audit_log(1).await.unwrap()[0]
        .detail
        .clone()
        .unwrap();
    assert!(
        audited.starts_with("capture on br-lan of ")
            && audited.ends_with(" (port 53), up to 1 s and 1 MiB"),
        "{audited}"
    );
    let pcap = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(pcap[..4], [0xd4, 0xc3, 0xb2, 0xa1]);
    assert_eq!(pcap[20..24], [1, 0, 0, 0], "Ethernet");
    assert!(pcap.len() > 24 + 16, "a packet within the second");
}

#[tokio::test]
async fn what_tcpdump_or_the_query_refuses_is_a_bad_request() {
    let demo = Demo::start().await;
    install_tcpdump(&demo).await;
    let node = demo.ids[0];
    for (query, detail) in [
        (
            "interface=lan9",
            "tcpdump refused it: lan9: No such device exists",
        ),
        ("filter=%28tcp", "The filter leaves a parenthesis open"),
        (
            "interface=-w",
            "\"-w\" is not a network device's name (or \"any\")",
        ),
        ("seconds=0", "A capture runs for 1 to 3600 seconds"),
    ] {
        let res = demo
            .send(demo.get(&format!("/v1/nodes/{node}/diag/capture?{query}")))
            .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{query}");
        assert_eq!(json(res).await["detail"], detail, "{query}");
    }
    let res = demo
        .send(demo.get(&format!("/v1/nodes/{node}/diag/capture?count=5")))
        .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST, "an unknown field");
    let res = demo.send(demo.get("/v1/nodes/999/diag/capture")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn one_capture_per_node_until_its_download_ends() {
    let demo = Demo::start().await;
    install_tcpdump(&demo).await;
    let uri = format!("/v1/nodes/{}/diag/capture", demo.ids[0]);
    let first = demo.send(demo.get(&uri)).await;
    assert_eq!(first.status(), StatusCode::OK);
    let res = demo.send(demo.get(&uri)).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    // Cancelling the download ends the capture, and frees the node for another.
    drop(first);
    let res = demo.send(demo.get(&uri)).await;
    assert_eq!(res.status(), StatusCode::OK);
}
