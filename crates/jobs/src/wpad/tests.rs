//! The wpad swap over the scripted node of [`crate::testing`] (24.10, opkg), adopted and managed.

use std::{sync::Arc, time::Duration};

use serde_json::{Value, json};
use uniwrt_core::NodeId;
use uniwrt_model::job::{Job, JobErrorCode, JobKind, JobState, StepName, StepState};
use uniwrt_node::{ExecOutput, ExitStatus, wpad::DIR};
use uniwrt_store::JobRepo as _;

use crate::{
    Error,
    testing::{COLLECTED, PASSWORD, Setup, setup, states},
};

const STATUS: &str = "Package: hostapd-common\nVersion: 2024.09.15~5ace39b0-r3\nStatus: install ok installed\n\
                      Package: wpad-basic-mbedtls\nVersion: 2024.09.15~5ace39b0-r3\nStatus: install user installed\n";
const DF: &str = "Filesystem           1K-blocks      Used Available Use% Mounted on\n\
                  overlayfs:/overlay       20000      1000     19000   5% /\n";

/// The capture with a hostapd, with 802.11v or without.
fn collected(wnm: bool) -> String {
    let mut value: Value = serde_json::from_str(COLLECTED).unwrap();
    let mut hostapd = vec!["sae", "11r"];
    if wnm {
        hostapd.push("11v");
    }
    value["sets"]["capabilities"]["hostapd"] = json!(hostapd);
    value.to_string()
}

fn ok(stdout: &str) -> ExecOutput {
    ExecOutput {
        status: ExitStatus::Code(0),
        stdout: stdout.as_bytes().to_vec(),
        stderr: Vec::new(),
    }
}

fn failed(stderr: &str) -> ExecOutput {
    ExecOutput {
        status: ExitStatus::Code(1),
        stdout: Vec::new(),
        stderr: stderr.as_bytes().to_vec(),
    }
}

/// How the node's side of the swap goes.
#[derive(Clone, Copy)]
struct Plays {
    /// What the swap writes to its state file.
    outcome: &'static str,
    log: &'static str,
    /// Reads of the result that fail with the login lost, once it's there.
    lost: usize,
    fetch_fails: bool,
}

const SWAPPED: Plays = Plays {
    outcome: "swapped",
    log: "result: swapped\n",
    lost: 0,
    fetch_fails: false,
};

async fn finished(setup: &Setup, job: Job) -> Job {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let job = setup.store.job(job.id).await.unwrap().unwrap();
            if job.state != JobState::Running {
                return job;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the job ends")
}

async fn adopted(manage: bool) -> (Setup, NodeId) {
    let setup = setup(true).await;
    let (job, _) = setup.adopt(setup.request(Some(PASSWORD))).await;
    let id = job.node.unwrap();
    if manage {
        let started = setup.jobs.manage(id, true, None).await.unwrap();
        assert_eq!(finished(&setup, started).await.state, JobState::Succeeded);
    }
    (setup, id)
}

fn play(setup: &Setup, plays: Plays) {
    *setup.node.collected.lock().unwrap() = Some(collected(false));
    let node = Arc::clone(&setup.node);
    let state_path = format!("{DIR}/state");
    let log_path = format!("{DIR}/log");
    let mut state = None;
    let mut lost = plays.lost;
    *setup.node.script.lock().unwrap() = Some(Box::new(move |argv: &[&str]| {
        Some(Ok(match argv {
            ["grep", _, _, "/usr/lib/opkg/status"] => ok(STATUS),
            ["df", "-k", _] => ok(DF),
            ["opkg", "update"] if plays.fetch_fails => {
                failed("Failed to download the package list from https://downloads.openwrt.org/\n")
            }
            [
                "sh",
                "-c",
                _,
                "sh",
                _,
                "opkg",
                "wpad-basic-mbedtls",
                "wpad-mbedtls",
                dir,
            ] if *dir == DIR => {
                state = Some(plays.outcome);
                if plays.outcome == "swapped" {
                    *node.collected.lock().unwrap() = Some(collected(true));
                }
                ok("started\n")
            }
            ["rm" | "mkdir", ..] | ["opkg", "update"] | ["sh", "-c", _, "sh", DIR, ..] => ok(""),
            ["cat", path] if *path == state_path => match state {
                Some(_) if lost > 0 => {
                    lost -= 1;
                    return Some(Err(uniwrt_node::Error::Disconnected {
                        what: "the session".to_owned(),
                    }));
                }
                Some(outcome) => ok(outcome),
                None => {
                    failed("cat: can't open '/tmp/uniwrt-wpad/state': No such file or directory")
                }
            },
            ["cat", path] if *path == log_path => ok(plays.log),
            _ => return None,
        }))
    }));
}

async fn swap(setup: &Setup, id: NodeId) -> Job {
    let started = setup.jobs.wpad(id, None).await.unwrap();
    assert_eq!(started.kind, JobKind::Wpad);
    finished(setup, started).await
}

fn ran(setup: &Setup, words: &[&str]) -> bool {
    setup
        .node
        .commands
        .lock()
        .unwrap()
        .iter()
        .any(|argv| argv.starts_with(&words.iter().map(|&w| w.to_owned()).collect::<Vec<_>>()))
}

fn launched(setup: &Setup) -> bool {
    setup
        .node
        .commands
        .lock()
        .unwrap()
        .iter()
        .any(|argv| argv.get(5).is_some_and(|word| word == "opkg"))
}

#[tokio::test]
async fn the_full_wpad_takes_the_basic_ones_place() {
    let (setup, id) = adopted(true).await;
    play(&setup, SWAPPED);
    let job = swap(&setup, id).await;
    assert_eq!(job.state, JobState::Succeeded, "{job:#?}");
    assert_eq!(
        states(&job),
        [
            (StepName::Connect, StepState::Done),
            (StepName::Check, StepState::Done),
            (StepName::Fetch, StepState::Done),
            (StepName::Swap, StepState::Done),
            (StepName::Verify, StepState::Done),
        ]
    );
    let check = job.steps[1].detail.as_deref().unwrap();
    assert_eq!(check, "wpad-mbedtls in place of wpad-basic-mbedtls");
    assert!(ran(&setup, &["opkg", "update"]));
    assert!(launched(&setup));
    // The packages go once 802.11v is there.
    let commands = setup.node.commands.lock().unwrap().clone();
    assert_eq!(commands.last().unwrap(), &["rm", "-rf", DIR]);
}

#[tokio::test]
async fn a_lost_login_is_logged_in_again_until_the_result_is_read() {
    let (setup, id) = adopted(true).await;
    play(&setup, Plays { lost: 1, ..SWAPPED });
    let job = swap(&setup, id).await;
    assert_eq!(job.state, JobState::Succeeded, "{job:#?}");
}

#[tokio::test]
async fn a_full_wpad_is_left_alone() {
    let (setup, id) = adopted(true).await;
    play(&setup, SWAPPED);
    *setup.node.collected.lock().unwrap() = Some(collected(true));
    let job = swap(&setup, id).await;
    assert_eq!(job.state, JobState::Succeeded, "{job:#?}");
    assert_eq!(
        job.steps[1].detail.as_deref(),
        Some("Its wpad has 802.11v already")
    );
    assert!(job.steps[2..].iter().all(|s| s.state == StepState::Skipped));
    assert!(!ran(&setup, &["opkg", "update"]));
}

#[tokio::test]
async fn a_failed_fetch_changes_nothing() {
    let (setup, id) = adopted(true).await;
    play(
        &setup,
        Plays {
            fetch_fails: true,
            ..SWAPPED
        },
    );
    let job = swap(&setup, id).await;
    assert_eq!(job.state, JobState::Failed);
    let error = job.error.unwrap();
    assert_eq!(error.code, JobErrorCode::NodeFailed);
    assert!(
        error
            .message
            .starts_with("Nothing changed. opkg update failed: Failed to download"),
        "{}",
        error.message
    );
    assert!(!launched(&setup));
    let commands = setup.node.commands.lock().unwrap().clone();
    assert_eq!(commands.last().unwrap(), &["rm", "-rf", DIR]);
}

#[tokio::test]
async fn a_swap_that_fails_says_which_wpad_runs_and_why() {
    let (setup, id) = adopted(true).await;
    play(
        &setup,
        Plays {
            outcome: "restored",
            log: "Installing wpad-mbedtls (2024.09.15~5ace39b0-r3) to root...\n\
                  Collected errors:\n * opkg_install_cmd: Cannot install package wpad-mbedtls.\n\
                  result: restored\n",
            ..SWAPPED
        },
    );
    let job = swap(&setup, id).await;
    assert_eq!(job.state, JobState::Failed);
    assert_eq!(job.steps[3].state, StepState::Failed);
    assert_eq!(
        job.error.unwrap().message,
        "wpad-mbedtls didn't install, so wpad-basic-mbedtls is back: \
         * opkg_install_cmd: Cannot install package wpad-mbedtls."
    );
}

#[tokio::test]
async fn only_managed_nodes_swap() {
    let (setup, id) = adopted(false).await;
    assert!(matches!(
        setup.jobs.wpad(id, None).await,
        Err(Error::Observed(node)) if node == id
    ));
}
