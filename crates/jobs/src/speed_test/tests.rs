//! Speed tests over the scripted gateway, whose script answers as a 100/20 Mbit/s line would:
//! 12 ms idle, more under load unless SQM shapes it.

use std::{fmt::Write as _, time::Duration};

use serde_json::json;
use uniwrt_model::{
    job::{Job, JobErrorCode, JobState, StepName, StepState},
    node::{NodeMode, NodeRole},
    speed::Grade,
};
use uniwrt_store::{NodeRepo as _, SpeedTestRepo as _};

use super::SpeedTestRequest;
use crate::{
    Error,
    apply::tests::fleet::{Fleet, fleet, output},
    testing::states,
};

const GW: (NodeRole, &str) = (NodeRole::Gateway, "192.168.1.1");

/// The script's output for `phase`: `kbps` on the loaded counter, replies of `loaded_ms`.
fn phase(phase: &str, kbps: u64, loaded_ms: f64) -> String {
    let mut out = String::new();
    if phase == "idle" {
        for i in 0..10 {
            let _ = writeln!(out, "{{\"rtt\":{}}}", 12.0 + f64::from(i % 3) * 0.1);
        }
        return out;
    }
    let per_s = kbps * 1000 / 8;
    for i in 0..=12u64 {
        let (rx, tx) = if phase == "down" {
            (i * per_s, 0)
        } else {
            (0, i * per_s)
        };
        let _ = writeln!(out, "{}", json!({"t": 5000 + i * 100, "rx": rx, "tx": tx}));
    }
    for _ in 0..12 {
        let _ = writeln!(out, "{{\"rtt\":{loaded_ms}}}");
    }
    out
}

fn line(fleet: &Fleet, wan_up: bool) {
    let fake = fleet.fake(0);
    *fake.collect_hook.lock().unwrap() = Some(Box::new(move |_, collected| {
        if !wan_up {
            return;
        }
        collected["sets"]["interfaces"]["dump"]["interface"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "interface": "wan", "up": true, "pending": false, "available": true,
                "autostart": true, "dynamic": false, "uptime": 10, "l3_device": "eth1.201",
                "proto": "dhcp", "device": "eth1.201",
                "ipv4-address": [{"address": "203.0.113.10", "mask": 24}],
                "ipv6-address": [], "ipv6-prefix": [], "ipv6-prefix-assignment": [],
                "route": [{"target": "0.0.0.0", "mask": 0, "nexthop": "203.0.113.1"}],
                "dns-server": [], "dns-search": [], "neighbors": [], "data": {},
            }));
    }));
    *fake.exec_hook.lock().unwrap() = Some(Box::new(|_, argv| {
        if argv[..3] != ["sh", "-s", "--"] {
            return None;
        }
        assert_eq!(argv[4], "eth1.201", "measured on the WAN's device");
        let bypass = argv[7] == "1";
        // Shaped, the queue stays short; the line alone bloats.
        let loaded = if bypass { 180.0 } else { 14.0 };
        Some(output(
            0,
            &match argv[3].as_str() {
                "idle" => phase("idle", 0, 0.0),
                "down" => phase("down", 100_000, loaded),
                _ => phase("up", 20_000, loaded),
            },
        ))
    }));
}

async fn run(fleet: &Fleet, bypass_sqm: bool) -> Job {
    let mut updates = fleet.jobs.subscribe();
    let started = fleet
        .jobs
        .speed_test(SpeedTestRequest { bypass_sqm }, None)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let job = updates.recv().await.unwrap();
            if job.id == started.id && job.state != JobState::Running {
                return (*job).clone();
            }
        }
    })
    .await
    .expect("the test ends")
}

#[tokio::test]
async fn a_speed_test_measures_the_line_and_keeps_the_result() {
    let fleet = fleet(&[GW]).await;
    line(&fleet, true);
    let job = run(&fleet, false).await;
    assert_eq!(job.state, JobState::Succeeded, "{:?}", job.error);
    assert!(
        states(&job)
            .iter()
            .all(|(_, state)| *state == StepState::Done)
    );
    let detail = |name| {
        job.steps
            .iter()
            .find(|s| s.name == name)
            .and_then(|s| s.detail.clone())
            .unwrap()
    };
    assert_eq!(detail(StepName::Idle), "12.1 ms");
    assert_eq!(
        detail(StepName::Download),
        "100.0 Mbit/s, 14.0 ms under load"
    );
    let tests = fleet.store.speed_tests(10).await.unwrap();
    assert_eq!(tests.len(), 1);
    let test = &tests[0];
    assert_eq!(
        (
            test.download_kbps,
            test.upload_kbps,
            test.idle_us,
            test.download_us
        ),
        (100_000, 20_000, 12_100, 14_000)
    );
    assert_eq!(
        (test.grade, test.bypassed, test.job),
        (Grade::APlus, false, job.id)
    );

    // The line alone, SQM stopped: bloated.
    let job = run(&fleet, true).await;
    assert_eq!(job.state, JobState::Succeeded, "{:?}", job.error);
    let tests = fleet.store.speed_tests(10).await.unwrap();
    assert_eq!((tests[0].grade, tests[0].bypassed), (Grade::C, true));
}

#[tokio::test]
async fn a_speed_test_needs_the_wan_up_and_a_managed_gateway_to_bypass_sqm() {
    let fleet = fleet(&[GW]).await;
    line(&fleet, false);
    let job = run(&fleet, false).await;
    assert_eq!(job.state, JobState::Failed);
    assert_eq!(job.error.unwrap().code, JobErrorCode::NoInternet);
    assert_eq!(fleet.store.speed_tests(10).await.unwrap(), []);

    let id = fleet.nodes[0].0.id;
    fleet
        .store
        .set_node_mode(id, NodeMode::Observe)
        .await
        .unwrap();
    assert!(matches!(
        fleet.jobs.speed_test(SpeedTestRequest { bypass_sqm: true }, None).await,
        Err(Error::Observed(node)) if node == id
    ));
}
