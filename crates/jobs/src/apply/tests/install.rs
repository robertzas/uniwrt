//! The apply's install step (D-076) over a scripted gateway: what SQM needs is installed before
//! anything is staged, its service kept off until the transaction switches it on; a failed
//! install changes nothing.

use std::sync::{Arc, Mutex};

use uniwrt_model::{
    job::{JobErrorCode, JobState, StepName, StepState},
    site::gateway::Sqm,
};

use super::fleet::{Fleet, commit, fleet, output};
use crate::testing::states;

const GW: (uniwrt_model::node::NodeRole, &str) =
    (uniwrt_model::node::NodeRole::Gateway, "192.168.1.1");

/// The commands the package manager ran on the gateway; `install` fails when `broken`.
fn script(fleet: &Fleet, broken: bool) -> Arc<Mutex<Vec<String>>> {
    let ran = Arc::new(Mutex::new(Vec::new()));
    let log = ran.clone();
    *fleet.fake(0).exec_hook.lock().unwrap() = Some(Box::new(move |tool, argv| {
        let line = argv.join(" ");
        match argv[0].as_str() {
            "grep" if argv.last().is_some_and(|f| f == "/usr/lib/opkg/status") => Some(output(
                0,
                "Package: base-files\nVersion: 1\nStatus: install user installed\n",
            )),
            "opkg" if argv[1] == "install" && broken => {
                log.lock().unwrap().push(line);
                Some(output(
                    255,
                    "Collected errors:\n * Unknown package 'sqm-scripts'.\n",
                ))
            }
            "opkg" => {
                if argv[1] == "install" {
                    // The package's postinst enables and starts its service.
                    tool.services.insert("sqm".to_owned(), true);
                }
                log.lock().unwrap().push(line);
                Some(output(0, ""))
            }
            "/etc/init.d/sqm" => {
                if argv[1] == "disable" {
                    tool.services.insert("sqm".to_owned(), false);
                }
                log.lock().unwrap().push(line);
                Some(output(0, ""))
            }
            _ => None,
        }
    }));
    ran
}

async fn with_sqm(fleet: &Fleet) {
    let head = fleet.head().await;
    let mut site = head.config.clone();
    site.wan.sqm = Some(Sqm {
        download_kbps: 90_000,
        upload_kbps: 18_000,
        overhead: Some(44),
        autotune: false,
    });
    commit(&fleet.store, Some(head.meta.id), site).await;
}

#[tokio::test]
async fn an_apply_installs_what_sqm_needs_before_staging() {
    let fleet = fleet(&[GW]).await;
    let ran = script(&fleet, false);
    with_sqm(&fleet).await;
    let plan = fleet.jobs.plan().await.unwrap();
    let gateway = &plan.nodes[0];
    assert_eq!(
        gateway.installs,
        ["sqm-scripts", "kmod-sched-cake"],
        "{gateway:?}"
    );
    assert!(gateway.services.iter().any(|s| s.name == "sqm" && s.enable));

    let job = fleet.apply().await;
    assert_eq!(job.state, JobState::Succeeded, "{:?}", job.error);
    assert!(states(&job).contains(&(StepName::Install, StepState::Done)));
    assert_eq!(
        *ran.lock().unwrap(),
        [
            "opkg update",
            "opkg install sqm-scripts kmod-sched-cake",
            "/etc/init.d/sqm stop",
            "/etc/init.d/sqm disable",
        ]
    );
    let sqm = fleet.fake(0).file("sqm").expect("the gateway holds sqm");
    assert!(sqm.contains("option download '90000'"), "{sqm}");
    assert_eq!(
        fleet.fake(0).tool.lock().unwrap().services.get("sqm"),
        Some(&true),
        "the transaction switched it on"
    );
}

#[tokio::test]
async fn a_failed_install_changes_nothing() {
    let fleet = fleet(&[GW]).await;
    let ran = script(&fleet, true);
    with_sqm(&fleet).await;
    let before = fleet.fake(0).tool.lock().unwrap().files.clone();
    let job = fleet.apply().await;
    assert_eq!(job.state, JobState::Failed);
    let error = job.error.as_ref().unwrap();
    assert_eq!(error.code, JobErrorCode::NodeFailed);
    assert!(
        error.message.contains("Unknown package 'sqm-scripts'"),
        "{}",
        error.message
    );
    assert!(states(&job).contains(&(StepName::Install, StepState::Failed)));
    assert!(states(&job).contains(&(StepName::Stage, StepState::Pending)));
    assert_eq!(ran.lock().unwrap().len(), 2);
    assert_eq!(fleet.fake(0).tool.lock().unwrap().files, before);
    assert!(fleet.journal().iter().all(|line| !line.contains("stage")));
}
