//! The DNS blocklist's push on the simulated gateway (D-133): uploads go to its `/tmp` once the
//! directory is there, the `services` set reports the list's header and the conf file, and
//! `rm -f` takes them away; uploads elsewhere are refused.

#![allow(clippy::unwrap_used)]

use std::time::Duration;

use bytes::Bytes;
use uniwrt_core::SystemClock;
use uniwrt_model::{
    blocklists::header,
    facts::{BlocklistFacts, NodeFacts},
    site::blocking::{BLOCKLIST_CONF, BLOCKLIST_FILE, CONF_DIR},
};
use uniwrt_node::{
    Error, ExecRequest, ExitStatus, NodeLink as _, facts,
    proto::ops::{Collect, Set},
};
use uniwrt_sim::{SimConfig, SimFleet, SimLink};

const WAIT: Duration = Duration::from_secs(5);

async fn blocklist(link: &SimLink) -> BlocklistFacts {
    let sets = Collect {
        sets: vec![Set::Services],
    };
    let mut found = NodeFacts::default();
    let _ = facts::apply(&link.request(&sets, WAIT).await.unwrap(), &mut found);
    found.services.blocklist
}

async fn run(link: &SimLink, argv: &[&str]) -> ExitStatus {
    let request = ExecRequest {
        argv: argv.iter().map(|&a| a.to_owned()).collect(),
        stdin: None,
        timeout: WAIT,
        max_output: 4096,
    };
    link.exec(request).await.unwrap().status
}

#[tokio::test(start_paused = true)]
async fn the_gateway_holds_the_pushed_list_until_it_is_removed() {
    let fleet = SimFleet::spawn(&SimConfig::default(), &SystemClock);
    let link = fleet.link(0).unwrap();
    assert_eq!(blocklist(&link).await, BlocklistFacts::default());

    let sha = "0a".repeat(32);
    let list = Bytes::from(format!("{}\nserver=/ads.example/\n", header(&sha, 1)));
    let missing = link.upload(BLOCKLIST_FILE, list.clone(), 0o644).await;
    assert!(matches!(missing, Err(Error::Remote { .. })), "{missing:?}");

    assert_eq!(
        run(&link, &["mkdir", "-p", CONF_DIR]).await,
        ExitStatus::Code(0)
    );
    link.upload(BLOCKLIST_FILE, list, 0o644).await.unwrap();
    let conf = Bytes::from(format!("servers-file={BLOCKLIST_FILE}\n"));
    link.upload(BLOCKLIST_CONF, conf, 0o644).await.unwrap();
    let signal = r#"{"name":"dnsmasq","signal":1}"#;
    let hup = ["ubus", "call", "service", "signal", signal];
    assert_eq!(run(&link, &hup).await, ExitStatus::Code(0));
    let restart = ["/etc/init.d/dnsmasq", "restart"];
    assert_eq!(run(&link, &restart).await, ExitStatus::Code(0));
    let held = BlocklistFacts {
        sha256: Some(sha),
        domains: Some(1),
        conf: true,
    };
    assert_eq!(blocklist(&link).await, held);

    let rm = ["rm", "-f", BLOCKLIST_CONF, BLOCKLIST_FILE];
    assert_eq!(run(&link, &rm).await, ExitStatus::Code(0));
    assert_eq!(blocklist(&link).await, BlocklistFacts::default());

    let elsewhere = link.upload("/etc/passwd", Bytes::new(), 0o644).await;
    assert!(
        matches!(elsewhere, Err(Error::Remote { .. })),
        "{elsewhere:?}"
    );
    assert_eq!(
        run(&link, &["rm", "-f", "/tmp/../etc/passwd"]).await,
        ExitStatus::Code(127)
    );
}
