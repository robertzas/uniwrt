//! The launcher with shell scripts for binaries: real processes, exit codes and signals; the
//! health probe looks for a file the script touches.

use std::{
    os::unix::fs::PermissionsExt as _,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use rustix::process::Signal;
use tokio::sync::mpsc;
use uniwrt_firmware::Version;

use super::{Bin, State, Supervisor, child::Timing};

const BUILTIN: Version = Version(0, 100, 0);
const NEWER: &str = "0.101.0";

struct Lab {
    dir: tempfile::TempDir,
}

impl Lab {
    fn new() -> Self {
        let lab = Self {
            dir: tempfile::tempdir().unwrap(),
        };
        std::fs::create_dir_all(lab.data().join("bin")).unwrap();
        lab
    }

    fn data(&self) -> PathBuf {
        self.dir.path().join("data")
    }

    fn bin(&self) -> Bin {
        Bin::new(&self.data())
    }

    fn script(path: &Path, body: &str) {
        std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// The image's own binary.
    fn own(&self, body: &str) {
        Self::script(&self.dir.path().join("own"), body);
    }

    /// A staged binary.
    fn staged(&self, version: &str, body: &str) {
        Self::script(
            &self.data().join("bin").join(format!("uniwrt-{version}")),
            body,
        );
    }

    fn healthy(&self) -> String {
        self.dir.path().join("healthy").display().to_string()
    }

    fn database(&self) -> PathBuf {
        self.data().join(crate::serve::DATABASE)
    }

    fn supervisor(&self) -> Supervisor {
        let healthy = PathBuf::from(self.healthy());
        Supervisor {
            bin: self.bin(),
            builtin: BUILTIN,
            own: self.dir.path().join("own"),
            args: vec!["serve".into()],
            probe: Arc::new(move || healthy.exists()),
            timing: Timing {
                trial: Duration::from_millis(600),
                soak: Duration::from_millis(100),
                probe_every: Duration::from_millis(20),
                stop_grace: Duration::from_secs(2),
            },
        }
    }

    fn set(&self, state: &State) {
        self.bin().save(state).unwrap();
    }

    fn state(&self) -> State {
        self.bin().load()
    }

    fn files_in_bin(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.data().join("bin"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }
}

/// Signals that never come, and a sender to send one.
fn signals() -> (mpsc::Sender<Signal>, mpsc::Receiver<Signal>) {
    mpsc::channel(4)
}

#[test]
fn the_database_files_are_the_stores() {
    assert_eq!(super::state::DATABASE_FILES[0], crate::serve::DATABASE);
}

#[tokio::test]
async fn without_a_staged_binary_the_images_runs_and_its_exit_code_is_passed_on() {
    let lab = Lab::new();
    lab.own("[ \"$1\" = serve ] && [ \"$UNIWRT_SUPERVISED\" = 1 ] && exit 7; exit 1");
    let (_tx, rx) = signals();
    assert_eq!(lab.supervisor().run(rx).await.unwrap(), 7);
}

#[tokio::test]
async fn a_trial_asked_for_is_started_confirmed_when_healthy_and_signals_are_forwarded() {
    let lab = Lab::new();
    std::fs::write(lab.database(), "before").unwrap();
    // The image's controller stages the trial, then asks to be started again.
    let state = lab.data().join("bin/state.json");
    let next = lab.dir.path().join("next");
    Lab::script(
        &next,
        &format!(
            "echo migrated > {}; touch {}; exec sleep 30",
            lab.database().display(),
            lab.healthy()
        ),
    );
    // As the controller does: stages the binary while it runs, names it, asks for a restart.
    lab.own(&format!(
        "cp -p {} {}/bin/uniwrt-{NEWER}; printf '{{\"trial\": \"{NEWER}\"}}' > {}; exit 75",
        next.display(),
        lab.data().display(),
        state.display()
    ));
    let (tx, rx) = signals();
    let supervisor = lab.supervisor();
    let run = tokio::spawn(async move { supervisor.run(rx).await });
    // Confirmed: the state names it current, the snapshot is gone.
    for _ in 0..100 {
        if lab.state().current.as_deref() == Some(NEWER) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(lab.state().current.as_deref(), Some(NEWER));
    assert_eq!(lab.state().trial, None);
    assert_eq!(lab.files_in_bin(), ["state.json", "uniwrt-0.101.0"]);
    tx.send(Signal::TERM).await.unwrap();
    assert_eq!(run.await.unwrap().unwrap(), 128 + 15);
    assert_eq!(
        std::fs::read_to_string(lab.database()).unwrap(),
        "migrated\n"
    );
}

#[tokio::test]
async fn a_trial_that_exits_is_rejected_and_the_database_put_back() {
    let lab = Lab::new();
    std::fs::write(lab.database(), "before").unwrap();
    lab.own("exit 0");
    lab.staged(
        NEWER,
        &format!(
            "echo migrated > {db}; echo wal > {db}-wal; exit 3",
            db = lab.database().display()
        ),
    );
    lab.set(&State {
        trial: Some(NEWER.to_owned()),
        ..State::default()
    });
    let (_tx, rx) = signals();
    assert_eq!(lab.supervisor().run(rx).await.unwrap(), 0);
    assert_eq!(std::fs::read_to_string(lab.database()).unwrap(), "before");
    assert!(!lab.data().join("uniwrt.db-wal").exists());
    let state = lab.state();
    assert_eq!(state.current, None);
    assert_eq!(state.rejected, [NEWER]);
    let failure = state.failure.unwrap();
    assert_eq!(failure.version, NEWER);
    assert!(
        failure.reason.contains("exited (status 3)"),
        "{}",
        failure.reason
    );
    assert_eq!(lab.files_in_bin(), ["state.json"]);
}

#[tokio::test]
async fn a_trial_never_healthy_is_stopped_and_rejected() {
    let lab = Lab::new();
    lab.own("exit 0");
    lab.staged(NEWER, "exec sleep 30");
    lab.set(&State {
        trial: Some(NEWER.to_owned()),
        ..State::default()
    });
    let (_tx, rx) = signals();
    assert_eq!(lab.supervisor().run(rx).await.unwrap(), 0);
    let failure = lab.state().failure.unwrap();
    assert!(
        failure.reason.contains("wasn't healthy"),
        "{}",
        failure.reason
    );
    // Never tried again, even when named again.
    lab.staged(NEWER, "exit 9");
    lab.set(&State {
        trial: Some(NEWER.to_owned()),
        ..lab.state()
    });
    let (_tx, rx) = signals();
    assert_eq!(lab.supervisor().run(rx).await.unwrap(), 0);
}

#[tokio::test]
async fn a_confirmed_binary_runs_until_the_image_is_newer() {
    let lab = Lab::new();
    lab.own("exit 5");
    lab.staged(NEWER, "exit 6");
    lab.staged("0.100.5", "exit 8");
    lab.set(&State {
        current: Some(NEWER.to_owned()),
        ..State::default()
    });
    let (_tx, rx) = signals();
    assert_eq!(lab.supervisor().run(rx).await.unwrap(), 6);
    // Binaries the state doesn't name are removed.
    assert_eq!(lab.files_in_bin(), ["state.json", "uniwrt-0.101.0"]);

    let mut newer_image = lab.supervisor();
    newer_image.builtin = Version(0, 102, 0);
    let (_tx, rx) = signals();
    assert_eq!(newer_image.run(rx).await.unwrap(), 5);
    assert_eq!(lab.state().current, None);
    assert_eq!(lab.files_in_bin(), ["state.json"]);
}

#[tokio::test]
async fn a_staged_binary_that_cant_run_falls_back() {
    let lab = Lab::new();
    lab.own("exit 0");
    std::fs::write(lab.data().join("bin/uniwrt-0.101.0"), "not executable").unwrap();
    lab.set(&State {
        current: Some(NEWER.to_owned()),
        ..State::default()
    });
    let (_tx, rx) = signals();
    assert_eq!(lab.supervisor().run(rx).await.unwrap(), 0);
    let failure = lab.state().failure.unwrap();
    assert!(failure.reason.contains("can't run"), "{}", failure.reason);
}
