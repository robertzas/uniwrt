//! The package manager's job over the scripted node of [`crate::testing`] (24.10, opkg),
//! adopted and managed, with opkg's words as 24.10 prints them.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use serde_json::Value;
use uniwrt_core::NodeId;
use uniwrt_model::{
    job::{Job, JobErrorCode, JobKind, JobState, StepName, StepState},
    packages::{PackageChanges, PackageLock},
};
use uniwrt_node::{ExecOutput, ExitStatus};
use uniwrt_store::{InventoryRepo as _, JobRepo as _};

use crate::{
    Error,
    testing::{COLLECTED, PASSWORD, Setup, setup, states},
};

const STATUS: &str = "Package: dropbear\nVersion: 2024.86-r3\nStatus: install ok installed\n\
                      Package: ucode-mod-ubus\nVersion: 2025.07.18~3f64c808-r1\nStatus: install ok installed\n\
                      Package: htop\nVersion: 3.3.0-r1\nStatus: install user installed\n\
                      Package: nano\nVersion: 8.2-r1\nStatus: install user installed\n\
                      Package: kernel\nVersion: 6.6.144~50daf-r1\nStatus: install ok installed\n";
const LISTS: &str = "Package: tcpdump\nVersion: 4.99.5-r1\nInstalled-Size: 901120\n\
                     Description:  Network monitoring and data acquisition tool\n\
                     Package: libpcap1\nVersion: 1.10.5-r2\nInstalled-Size: 300000\nDescription:  libpcap\n\
                     Package: nano\nVersion: 8.3-r1\nInstalled-Size: 200000\nDescription:  An editor\n";
const UPGRADABLE: &str = "nano - 8.2-r1 - 8.3-r1\nkernel - 6.6.144~50daf-r1 - 6.6.145~1-r1\n";

fn ok(stdout: &str) -> ExecOutput {
    ExecOutput {
        status: ExitStatus::Code(0),
        stdout: stdout.as_bytes().to_vec(),
        stderr: Vec::new(),
    }
}

fn failed(code: u32, stderr: &str) -> ExecOutput {
    ExecOutput {
        status: ExitStatus::Code(code),
        stdout: Vec::new(),
        stderr: stderr.as_bytes().to_vec(),
    }
}

/// How opkg answers, by dry run.
#[derive(Clone, Copy)]
struct Opkg {
    remove: &'static str,
    install_fails: bool,
}

const PLAIN: Opkg = Opkg {
    remove: "Removing package htop from root...\n",
    install_fails: false,
};

fn play(setup: &Setup, opkg: Opkg) -> Arc<Mutex<usize>> {
    let reads = Arc::new(Mutex::new(0));
    let counted = Arc::clone(&reads);
    *setup.node.script.lock().unwrap() = Some(Box::new(move |argv: &[&str]| {
        Some(Ok(match argv {
            ["grep", _, _, "/usr/lib/opkg/status"] => {
                *counted.lock().unwrap() += 1;
                ok(STATUS)
            }
            ["grep", _, _, "/rom/usr/lib/opkg/status"] => {
                failed(2, "grep: /rom/usr/lib/opkg/status: No such file")
            }
            ["sh", "-c", script] if script.contains("opkg-lists") => ok(LISTS),
            ["opkg", "list-upgradable"] => ok(UPGRADABLE),
            ["opkg", "remove", "--noaction", "--autoremove", ..] => {
                if opkg.remove.contains("print_dependents_warning") {
                    ExecOutput {
                        status: ExitStatus::Code(255),
                        stdout: opkg.remove.as_bytes().to_vec(),
                        stderr: Vec::new(),
                    }
                } else {
                    ok(opkg.remove)
                }
            }
            ["opkg", "upgrade", "--noaction", "nano"] => {
                ok("Upgrading nano on root from 8.2-r1 to 8.3-r1...\n")
            }
            ["opkg", "install", "--noaction", "tcpdump"] => ok(
                "Installing tcpdump (4.99.5-r1) to root...\nInstalling libpcap1 (1.10.5-r2) to root...\n",
            ),
            ["opkg", "install", "tcpdump"] if opkg.install_fails => failed(
                255,
                "Collected errors:\n * opkg_download: Failed to download tcpdump, wget returned 4.\n",
            ),
            // The other changes, and the managed packages read for render (none here).
            ["opkg", "update" | "remove" | "upgrade" | "install", ..] | ["sh", "-c", ..] => ok(""),
            _ => return None,
        }))
    }));
    reads
}

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

fn changes(install: &[&str], remove: &[&str], upgrade: &[&str]) -> PackageChanges {
    let names = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect();
    PackageChanges {
        install: names(install),
        remove: names(remove),
        upgrade: names(upgrade),
    }
}

async fn change(setup: &Setup, id: NodeId, asked: PackageChanges) -> Job {
    let started = setup.jobs.packages(id, asked, None).await.unwrap();
    assert_eq!(started.kind, JobKind::Packages);
    finished(setup, started).await
}

/// Every command run, words joined.
fn lines(setup: &Setup) -> Vec<String> {
    setup
        .node
        .commands
        .lock()
        .unwrap()
        .iter()
        .map(|argv| argv.join(" "))
        .collect()
}

/// The commands that changed packages, in order.
fn changed(setup: &Setup) -> Vec<String> {
    lines(setup)
        .into_iter()
        .filter(|line| {
            ["opkg remove", "opkg upgrade", "opkg install"]
                .iter()
                .any(|verb| line.starts_with(verb))
                && !line.contains("--noaction")
        })
        .collect()
}

#[tokio::test]
async fn removals_go_first_then_upgrades_then_installs() {
    let (setup, id) = adopted(true).await;
    let reads = play(&setup, PLAIN);
    let job = change(&setup, id, changes(&["tcpdump"], &["htop"], &["nano"])).await;
    assert_eq!(job.state, JobState::Succeeded, "{job:#?}");
    assert_eq!(
        states(&job),
        [
            (StepName::Connect, StepState::Done),
            (StepName::Check, StepState::Done),
            (StepName::Install, StepState::Done),
            (StepName::Inventory, StepState::Done),
        ]
    );
    assert_eq!(
        job.steps[1].detail.as_deref(),
        Some("Removes htop; upgrades nano; installs tcpdump, libpcap1 (1.1 MiB)")
    );
    assert_eq!(
        job.steps[2].detail.as_deref(),
        Some("Removed htop; upgraded nano; installed tcpdump")
    );
    assert_eq!(
        changed(&setup),
        [
            "opkg remove --autoremove htop",
            "opkg upgrade nano",
            "opkg install tcpdump"
        ]
    );
    // Read at the check and kept after the change.
    assert_eq!(*reads.lock().unwrap(), 2);
    let kept = setup.store.inventory(id).await.unwrap().unwrap();
    assert!(
        kept.packages.iter().any(|p| p.name == "htop"),
        "as the node said"
    );
}

#[tokio::test]
async fn locked_and_unknown_packages_are_refused_before_the_index_is_read() {
    let (setup, id) = adopted(true).await;
    play(&setup, PLAIN);
    for (asked, why) in [
        (
            changes(&[], &["dropbear"], &[]),
            "dropbear is locked: the device needs it",
        ),
        (changes(&[], &["vim"], &[]), "vim isn't installed"),
        (changes(&["nano"], &[], &[]), "nano is installed already"),
        (
            changes(&[], &[], &["kernel"]),
            "kernel comes with the firmware: upgrade the firmware instead",
        ),
        (changes(&["htop"], &["htop"], &[]), "htop is named twice"),
    ] {
        let job = change(&setup, id, asked).await;
        assert_eq!(job.state, JobState::Failed);
        let error = job.error.unwrap();
        assert_eq!(error.code, JobErrorCode::Refused);
        assert_eq!(error.message, why);
    }
    assert!(!lines(&setup).iter().any(|line| line == "opkg update"));
    assert_eq!(changed(&setup), Vec::<String>::new());
}

#[tokio::test]
async fn the_dry_run_refuses_what_would_break_the_node() {
    let (setup, id) = adopted(true).await;
    for (opkg, asked, why) in [
        (
            Opkg {
                remove: "Removing package htop from root...\nRemoving package ucode-mod-ubus from root...\n",
                ..PLAIN
            },
            changes(&[], &["htop"], &[]),
            "That would remove ucode-mod-ubus along, which the device needs",
        ),
        (
            Opkg {
                remove: "No packages removed.\nCollected errors:\n \
                         * print_dependents_warning: Package htop is depended upon by packages:\n \
                         * print_dependents_warning: \tnano\n",
                ..PLAIN
            },
            changes(&[], &["htop"], &[]),
            "htop can't go: nano needs it",
        ),
        (
            PLAIN,
            changes(&["vim"], &[], &[]),
            "vim isn't in the device's package feeds",
        ),
    ] {
        play(&setup, opkg);
        let job = change(&setup, id, asked).await;
        assert_eq!(job.state, JobState::Failed);
        assert_eq!(job.error.unwrap().message, why);
    }
    assert_eq!(changed(&setup), Vec::<String>::new());
}

#[tokio::test]
async fn new_packages_must_leave_flash_to_spare() {
    let (setup, id) = adopted(true).await;
    play(&setup, PLAIN);
    let mut collected: Value = serde_json::from_str(COLLECTED).unwrap();
    // 8 MiB, 2 free (KiB, as ubus says).
    collected["sets"]["system"]["info"]["root"]["total"] = 8192.into();
    collected["sets"]["system"]["info"]["root"]["avail"] = 2000.into();
    *setup.node.collected.lock().unwrap() = Some(collected.to_string());
    let job = change(&setup, id, changes(&["tcpdump"], &[], &[])).await;
    assert_eq!(job.state, JobState::Failed);
    assert_eq!(
        job.error.unwrap().message,
        "Nothing changed. That needs 1.1 MiB of flash, and the device has 2.0 MiB free (keeping 1.0 MiB spare)"
    );
    assert_eq!(changed(&setup), Vec::<String>::new());
}

#[tokio::test]
async fn a_failed_install_says_what_changed_and_reads_the_packages_again() {
    let (setup, id) = adopted(true).await;
    let reads = play(
        &setup,
        Opkg {
            install_fails: true,
            ..PLAIN
        },
    );
    let job = change(&setup, id, changes(&["tcpdump"], &["htop"], &[])).await;
    assert_eq!(job.state, JobState::Failed);
    let error = job.error.unwrap();
    assert_eq!(error.code, JobErrorCode::NodeFailed);
    assert_eq!(
        error.message,
        "Installing tcpdump failed (removed htop first): * opkg_download: Failed to download tcpdump, wget returned 4."
    );
    assert_eq!(*reads.lock().unwrap(), 2);
}

#[tokio::test]
async fn only_managed_nodes_and_package_names_are_taken() {
    let (setup, id) = adopted(false).await;
    let err = setup
        .jobs
        .packages(id, changes(&["tcpdump"], &[], &[]), None)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Observed(_)), "{err:?}");
    let err = setup.jobs.package_catalog(id).await.unwrap_err();
    assert!(matches!(err, Error::Observed(_)), "{err:?}");
    for asked in [
        changes(&["--force-depends"], &[], &[]),
        PackageChanges::default(),
    ] {
        let err = setup.jobs.packages(id, asked, None).await.unwrap_err();
        assert!(matches!(err, Error::Refused(_)), "{err:?}");
    }
}

#[tokio::test]
async fn the_inventory_shows_each_package_s_lock_and_the_catalog_its_upgrades() {
    let (setup, id) = adopted(true).await;
    play(&setup, PLAIN);
    // The adoption read the testing node's status; read this one's.
    setup
        .jobs
        .read_packages(id, uniwrt_model::facts::PackageManager::Opkg)
        .await
        .unwrap();
    let listed = setup.jobs.installed_packages(id).await.unwrap();
    assert!(listed.taken_at.is_some());
    assert_eq!(listed.required_unknown, None);
    let lock = |name: &str| {
        let package = listed.packages.iter().find(|p| p.name == name).unwrap();
        (package.lock, package.with_firmware)
    };
    assert_eq!(lock("dropbear"), (Some(PackageLock::Essential), false));
    assert_eq!(lock("htop"), (None, false));
    assert_eq!(lock("kernel"), (Some(PackageLock::Essential), true));

    let catalog = setup.jobs.package_catalog(id).await.unwrap();
    assert_eq!(
        catalog
            .available
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["libpcap1", "nano", "tcpdump"]
    );
    assert_eq!(catalog.upgrades.len(), 2);
    assert_eq!(
        (
            catalog.upgrades[1].name.as_str(),
            catalog.upgrades[1].to.as_str()
        ),
        ("nano", "8.3-r1")
    );
    assert!(lines(&setup).iter().any(|line| line == "opkg update"));
}
