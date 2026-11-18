//! The upgrade over the scripted node of [`crate::testing`], adopted and managed: the image
//! streamed into its files, sysupgrade scripted to take it down for a few tries and bring it back
//! with the facts of the new release.

use std::{
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

use uniwrt_core::NodeId;
use uniwrt_model::{
    firmware::FirmwareImage,
    inventory::BackupReason,
    job::{Job, JobErrorCode, JobState, StepName, StepState},
};
use uniwrt_node::{ExecOutput, ExitStatus};
use uniwrt_store::{InventoryRepo as _, JobRepo as _};

use super::{FetchImage, STAGED, UpgradeRequest, UpgradeTiming};
use crate::{
    ApplyTiming, BoxFuture, Error,
    testing::{COLLECTED, FakeNode, PASSWORD, Setup, setup_timed, states},
};

pub(crate) const IMAGE: &[u8] = b"an OpenWrt 25.12.6 sysupgrade image";
const SHA256: &str = "5ad1e7c0ffee5ad1e7c0ffee5ad1e7c0ffee5ad1e7c0ffee5ad1e7c0ffee5ad1";

/// How the scripted node takes the flash.
#[derive(Clone, Copy)]
pub(crate) struct Flash {
    /// `sysupgrade -T` refuses the image.
    refuse: bool,
    /// Tries it stays down after sysupgrade; `u32::MAX`: it never comes back.
    down: u32,
    /// The release it runs after.
    release: &'static str,
    /// The transaction tool survives the flash.
    keeps_tool: bool,
}

pub(crate) const GOOD: Flash = Flash {
    refuse: false,
    down: 3,
    release: "25.12.6",
    keeps_tool: true,
};

fn ok(stdout: &[u8]) -> ExecOutput {
    ExecOutput {
        status: ExitStatus::Code(0),
        stdout: stdout.to_vec(),
        stderr: Vec::new(),
    }
}

fn failed(stderr: &[u8]) -> ExecOutput {
    ExecOutput {
        status: ExitStatus::Code(1),
        stdout: Vec::new(),
        stderr: stderr.to_vec(),
    }
}

pub(crate) fn play(node: &Arc<FakeNode>, flash: Flash) {
    let this = Arc::clone(node);
    *node.script.lock().unwrap() = Some(Box::new(move |argv| {
        let staged = |path: &str| this.files.lock().unwrap().get(path).cloned();
        Some(Ok(match argv {
            ["sha256sum", path] => match staged(path) {
                Some(data) if data == IMAGE => ok(format!("{SHA256}  {path}\n").as_bytes()),
                Some(_) => ok(format!("{}  {path}\n", "0".repeat(64)).as_bytes()),
                None => failed(b"sha256sum: No such file or directory"),
            },
            ["sysupgrade", "-T", path] if staged(path).is_some() && !flash.refuse => ok(b""),
            ["sysupgrade", "-T", _] => failed(b"Device container not supported by this image"),
            ["sysupgrade", path] if staged(path).is_some() => {
                this.files.lock().unwrap().clear();
                this.down.store(flash.down, Ordering::SeqCst);
                *this.collected.lock().unwrap() = Some(COLLECTED.replace("24.10.8", flash.release));
                if !flash.keeps_tool {
                    *this.footprint.lock().unwrap() = None;
                }
                return Some(Err(uniwrt_node::Error::Disconnected {
                    what: "the session".to_owned(),
                }));
            }
            ["rm", "-f", path] => {
                this.files.lock().unwrap().remove(*path);
                ok(b"")
            }
            ["opkg", "install", rest @ ..] if rest.contains(&"missing-pkg") => {
                failed(b"Unknown package 'missing-pkg'.")
            }
            ["opkg", "update" | "install" | "remove", ..] => ok(b""),
            _ => return None,
        }))
    }));
}

pub(crate) struct Fetch(pub(crate) Option<PathBuf>);

impl FetchImage for Fetch {
    fn fetch<'a>(&'a self, _image: &'a FirmwareImage) -> BoxFuture<'a, Result<PathBuf, String>> {
        Box::pin(async move {
            self.0
                .clone()
                .ok_or_else(|| "ASU's store answered 404".to_owned())
        })
    }
}

pub(crate) fn timing() -> ApplyTiming {
    ApplyTiming {
        upgrade: UpgradeTiming {
            poll: Duration::from_millis(5),
            down: Duration::from_millis(200),
            back: Duration::from_millis(300),
            settle: Duration::from_millis(20),
            soak: Duration::from_millis(20),
            soak_every: Duration::from_millis(10),
        },
        ..ApplyTiming::default()
    }
}

pub(crate) async fn managed(setup: &Setup) -> NodeId {
    let (job, _) = setup.adopt(setup.request(Some(PASSWORD))).await;
    let id = job.node.unwrap();
    let job = setup.jobs.manage(id, true, None).await.unwrap();
    assert_eq!(finished(setup, job).await.state, JobState::Succeeded);
    id
}

pub(crate) async fn finished(setup: &Setup, job: Job) -> Job {
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

pub(crate) fn request(node: NodeId, image: Option<PathBuf>, packages: &[&str]) -> UpgradeRequest {
    UpgradeRequest {
        node,
        from: "24.10.8".to_owned(),
        to: Some("25.12.6".to_owned()),
        images: vec![FirmwareImage {
            name: "openwrt-25.12.6-x86-64-generic-squashfs-combined.img.gz".to_owned(),
            kind: "combined".to_owned(),
            filesystem: Some("squashfs".to_owned()),
            size_bytes: Some(IMAGE.len() as u64),
            sha256: SHA256.to_owned(),
            url: "https://downloads.openwrt.org/x".to_owned(),
        }],
        packages: packages.iter().map(|name| (*name).to_owned()).collect(),
        fetch: Arc::new(Fetch(image)),
    }
}

/// Upgrades the managed node with `flash` played; the job as it ends.
async fn upgrade(flash: Flash, packages: &[&str]) -> (Setup, Job, tempfile::TempDir) {
    let setup = setup_timed(true, timing()).await;
    let id = managed(&setup).await;
    play(&setup.node, flash);
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("image.img");
    std::fs::write(&image, IMAGE).unwrap();
    let job = setup
        .jobs
        .upgrade(request(id, Some(image), packages), None)
        .await
        .unwrap();
    let job = finished(&setup, job).await;
    (setup, job, dir)
}

fn step(job: &Job, name: StepName) -> (StepState, String) {
    let step = job.steps.iter().find(|step| step.name == name).unwrap();
    (step.state, step.detail.clone().unwrap_or_default())
}

fn ran(setup: &Setup, argv: &[&str]) -> bool {
    setup
        .node
        .commands
        .lock()
        .unwrap()
        .iter()
        .any(|command| command.iter().map(String::as_str).eq(argv.iter().copied()))
}

#[tokio::test]
async fn the_image_is_staged_checked_flashed_and_the_node_checked_after() {
    let (setup, job, _dir) = upgrade(
        Flash {
            keeps_tool: false,
            ..GOOD
        },
        &["luci", "kmod-batman-adv", "missing-pkg", "-dropbear"],
    )
    .await;
    assert_eq!(job.state, JobState::Succeeded, "{job:#?}");
    assert_eq!(job.target, "OpenWrt to 25.12.6");
    assert!(states(&job).iter().all(|(name, state)| *state
        == if *name == StepName::Install {
            StepState::Failed
        } else {
            StepState::Done
        }));
    assert!(ran(&setup, &["dd", &format!("of={STAGED}"), "bs=64k"]));
    assert!(ran(&setup, &["sysupgrade", "-T", STAGED]));
    assert!(
        ran(&setup, &["sysupgrade", STAGED]),
        "keeping the configuration"
    );

    let id = job.node.unwrap();
    let backups = setup.store.backups(id).await.unwrap();
    assert_eq!(backups[0].reason, BackupReason::Upgrade);
    assert_eq!(backups.len(), 2, "the adoption's and the upgrade's");

    // An official image's packages: what fails is a warning.
    let (_, install) = step(&job, StepName::Install);
    assert!(install.starts_with("1 installed, 1 removed"), "{install}");
    assert!(install.contains("missing-pkg not installed"), "{install}");
    assert!(ran(&setup, &["opkg", "remove", "dropbear"]));
    let (_, verify) = step(&job, StepName::Verify);
    assert!(verify.starts_with("Runs 25.12.6"), "{verify}");
    assert!(
        verify.contains("transaction tool installed again"),
        "{verify}"
    );
    assert!(setup.node.footprint.lock().unwrap().is_some());
    assert!(step(&job, StepName::Reboot).1.starts_with("Back after"));
    assert!(setup.store.inventory(id).await.unwrap().is_some());
}

#[tokio::test]
async fn an_image_sysupgrade_refuses_changes_nothing() {
    let (setup, job, _dir) = upgrade(
        Flash {
            refuse: true,
            ..GOOD
        },
        &[],
    )
    .await;
    assert_eq!(job.state, JobState::Failed);
    let error = job.error.clone().unwrap();
    assert_eq!(error.code, JobErrorCode::Refused);
    assert!(
        error.message.contains("not supported by this image"),
        "{error:?}"
    );
    assert!(!ran(&setup, &["sysupgrade", STAGED]));
    assert!(
        setup.node.files.lock().unwrap().is_empty(),
        "the image is deleted"
    );
    assert_eq!(step(&job, StepName::Flash).0, StepState::Pending);
}

#[tokio::test]
async fn a_node_that_doesnt_come_back_says_how_to_recover() {
    let (_, job, _dir) = upgrade(
        Flash {
            down: u32::MAX,
            ..GOOD
        },
        &[],
    )
    .await;
    let error = job.error.clone().unwrap();
    assert_eq!(error.code, JobErrorCode::NotBack, "{job:#?}");
    assert!(error.message.contains("failsafe"), "{error:?}");
    assert_eq!(step(&job, StepName::Reboot).0, StepState::Failed);
}

#[tokio::test]
async fn a_node_back_on_another_release_fails_and_says_its_flashed() {
    let (_, job, _dir) = upgrade(
        Flash {
            release: "24.10.9",
            ..GOOD
        },
        &[],
    )
    .await;
    let error = job.error.clone().unwrap();
    assert_eq!(error.code, JobErrorCode::NodeFailed);
    assert!(
        error.message.contains("runs 24.10.9, not 25.12.6"),
        "{error:?}"
    );
    assert!(error.message.contains("backed up"), "{error:?}");
    assert_eq!(step(&job, StepName::Install).0, StepState::Skipped);
}

#[tokio::test]
async fn nothing_happens_on_another_release_an_observed_node_or_without_the_image() {
    let setup = setup_timed(true, timing()).await;
    let (adopted, _) = setup.adopt(setup.request(Some(PASSWORD))).await;
    let observed = adopted.node.unwrap();
    assert!(matches!(
        setup.jobs.upgrade(request(observed, None, &[]), None).await,
        Err(Error::Observed(_))
    ));
    let id = {
        let job = setup.jobs.manage(observed, true, None).await.unwrap();
        finished(&setup, job).await.node.unwrap()
    };
    play(&setup.node, GOOD);

    let mut stale = request(id, None, &[]);
    stale.from = "24.10.7".to_owned();
    let job = finished(&setup, setup.jobs.upgrade(stale, None).await.unwrap()).await;
    assert_eq!(job.error.as_ref().unwrap().code, JobErrorCode::Refused);
    assert!(job.error.unwrap().message.contains("runs 24.10.8 now"));
    assert_eq!(setup.store.backups(id).await.unwrap().len(), 1, "no backup");

    let job = finished(
        &setup,
        setup
            .jobs
            .upgrade(request(id, None, &[]), None)
            .await
            .unwrap(),
    )
    .await;
    let error = job.error.unwrap();
    assert_eq!(error.code, JobErrorCode::Refused);
    assert!(error.message.contains("404"), "{error:?}");
    assert!(!ran(&setup, &["sysupgrade", "-T", STAGED]));
}
