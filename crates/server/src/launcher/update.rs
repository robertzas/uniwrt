//! The controller's own install (M7.1b2, D-123): [`Updater`] is the API's [`SelfUpdate`] and
//! the automatic install's. An install runs in the background: the release's tarball staged in
//! `<data>/bin` and verified ([`GitHub::stage_controller`] with [`sigstore::verify`] against
//! the embedded trust root and uniwrt's CI), its binary run once (`version --json` must name
//! the release and this target), then named the trial in `state.json` and the controller
//! stopped with [`RESTART`](super::RESTART) through [`Restart`]: the launcher starts the trial.
//! Only a controller the launcher runs installs; others say why not ([`Updater::blocked`]).

use std::{
    io,
    path::Path,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::{info, warn};
use uniwrt_api::controller_update::{SelfUpdate, Trials};
use uniwrt_core::Timestamp;
use uniwrt_firmware::{
    ControllerRelease, GitHub, Version,
    sigstore::{self, Identity, TrustedRoot},
};
use uniwrt_model::controller::{InstallProgress, TrialFailure};

use super::{Bin, SUPERVISED};
use crate::{build_info, cli::ServeArgs};

/// How long the new binary may take to answer `version --json`.
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);

/// Stops `serve` so the launcher starts the controller again.
#[derive(Clone)]
pub(crate) struct Restart {
    stop: mpsc::Sender<&'static str>,
    asked: Arc<AtomicBool>,
}

impl Restart {
    /// Asks through `stop`, `serve`'s stop requests.
    pub(crate) fn new(stop: mpsc::Sender<&'static str>) -> Self {
        Self {
            stop,
            asked: Arc::default(),
        }
    }

    pub(crate) fn ask(&self) {
        self.asked.store(true, Ordering::SeqCst);
        // A full channel holds stop requests already: `serve` stops anyway.
        let _ = self.stop.try_send("restarting into a new release");
    }

    /// Whether `serve` should exit with [`RESTART`](super::RESTART).
    pub(crate) fn asked(&self) -> bool {
        self.asked.load(Ordering::SeqCst)
    }
}

/// The controller's installer. Clones share it.
#[derive(Clone)]
pub(crate) struct Updater {
    inner: Arc<Inner>,
}

struct Inner {
    github: GitHub,
    bin: Bin,
    blocked: Option<String>,
    progress: Mutex<InstallProgress>,
    restart: Restart,
}

impl Updater {
    pub(crate) fn new(github: GitHub, data: &Path, args: &ServeArgs, restart: Restart) -> Self {
        Self {
            inner: Arc::new(Inner {
                github,
                bin: Bin::new(data),
                blocked: Self::blocked(build_info::VERSION, args.demo, supervised()),
                progress: Mutex::default(),
                restart,
            }),
        }
    }

    /// Why a controller of `version` can't install releases itself: only one the launcher runs
    /// (a release build, not in demo mode, self-upgrade on) can.
    pub(crate) fn blocked(version: &str, demo: bool, supervised: bool) -> Option<String> {
        if version.parse::<Version>().is_err() {
            Some(format!(
                "This is a development build ({version}): it doesn't install releases"
            ))
        } else if demo {
            Some("Demo mode doesn't install releases".to_owned())
        } else if !supervised {
            Some(
                "Self-upgrade is off (--self-upgrade off): update the container's image instead"
                    .to_owned(),
            )
        } else {
            None
        }
    }

    fn set(&self, progress: InstallProgress) {
        *self
            .inner
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = progress;
    }

    async fn run(&self, release: &ControllerRelease) -> Result<(), String> {
        let version = release.version;
        let root = TrustedRoot::embedded()
            .map_err(|err| format!("the embedded Sigstore trust root is unusable: {err}"))?;
        let staged = self
            .inner
            .github
            .stage_controller(
                release,
                build_info::TARGET,
                &self.inner.bin.dir,
                |bundle, subject| sigstore::verify(bundle, subject, Identity::UNIWRT_CI, &root),
            )
            .await
            .map_err(|err| err.to_string())?;
        info!(
            %version,
            commit = staged.provenance.commit.as_deref().unwrap_or("unknown"),
            "staged the controller's release; its attestation holds"
        );
        self.set(InstallProgress::Checking {
            version: version.to_string(),
        });
        if let Err(why) = check(&staged.path, version).await {
            let _ = tokio::fs::remove_file(&staged.path).await;
            return Err(why);
        }
        let bin = &self.inner.bin;
        let mut state = bin.load();
        state.trial = Some(version.to_string());
        bin.save(&state)
            .map_err(|err| format!("cannot write the launcher's state: {err}"))
    }
}

/// Whether the launcher runs this controller.
fn supervised() -> bool {
    std::env::var_os(SUPERVISED).is_some()
}

/// Runs the staged binary once: `version --json` must name `version` and this target.
async fn check(path: &Path, version: Version) -> Result<(), String> {
    #[derive(Deserialize)]
    struct Info {
        version: String,
        target: String,
    }
    let run = tokio::process::Command::new(path)
        .args(["version", "--json"])
        .env_remove(SUPERVISED)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    let out = tokio::time::timeout(CHECK_TIMEOUT, run)
        .await
        .map_err(|_| format!("the new binary didn't answer within {CHECK_TIMEOUT:?}"))?
        .map_err(|err| match err.kind() {
            io::ErrorKind::PermissionDenied => {
                format!("the new binary can't run: {err} (is the data directory mounted noexec?)")
            }
            _ => format!("the new binary can't run: {err}"),
        })?;
    if !out.status.success() {
        return Err(format!(
            "the new binary's `version` failed ({})",
            out.status
        ));
    }
    let info: Info = serde_json::from_slice(&out.stdout)
        .map_err(|err| format!("the new binary's `version --json` is unreadable: {err}"))?;
    if info.version != version.to_string() || info.target != build_info::TARGET {
        return Err(format!(
            "the new binary is {} for {}, not {version} for {}",
            info.version,
            info.target,
            build_info::TARGET
        ));
    }
    Ok(())
}

impl SelfUpdate for Updater {
    fn target(&self) -> &'static str {
        build_info::TARGET
    }

    fn blocked(&self) -> Option<String> {
        self.inner.blocked.clone()
    }

    fn trials(&self) -> Trials {
        let state = self.inner.bin.load();
        Trials {
            failure: state.failure.map(|failure| TrialFailure {
                version: failure.version,
                reason: failure.reason,
                at: Timestamp::from_unix_ms(failure.at.saturating_mul(1_000)),
            }),
            rejected: state
                .rejected
                .iter()
                .filter_map(|version| version.parse().ok())
                .collect(),
        }
    }

    fn progress(&self) -> InstallProgress {
        self.inner
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn install(&self, release: ControllerRelease) -> Result<(), String> {
        if let Some(why) = &self.inner.blocked {
            return Err(why.clone());
        }
        {
            let mut progress = self
                .inner
                .progress
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if progress.busy() {
                return Err("An install is already under way".to_owned());
            }
            *progress = InstallProgress::Downloading {
                version: release.version.to_string(),
            };
        }
        info!(version = %release.version, "installing the controller's release");
        let updater = self.clone();
        tokio::spawn(async move {
            let version = release.version.to_string();
            match updater.run(&release).await {
                Ok(()) => {
                    info!(%version, "restarting into the new release");
                    updater.set(InstallProgress::Restarting { version });
                    updater.inner.restart.ask();
                }
                Err(reason) => {
                    warn!(%version, %reason, "cannot install the controller's release");
                    updater.set(InstallProgress::Failed { version, reason });
                }
            }
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;

    use super::*;

    fn script(dir: &Path, name: &str, body: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn only_a_supervised_release_build_installs() {
        assert!(
            Updater::blocked("0.0.0-dev.3+abc", false, true)
                .unwrap()
                .contains("development")
        );
        assert!(
            Updater::blocked("1.2.3", true, true)
                .unwrap()
                .contains("Demo")
        );
        assert!(
            Updater::blocked("1.2.3", false, false)
                .unwrap()
                .contains("--self-upgrade off")
        );
        assert_eq!(Updater::blocked("1.2.3", false, true), None);
    }

    #[tokio::test]
    async fn the_new_binary_must_name_the_release_and_this_target() {
        let dir = tempfile::tempdir().unwrap();
        let target = build_info::TARGET;
        let good = script(
            dir.path(),
            "good",
            &format!(
                r#"[ "$1 $2" = "version --json" ] && echo '{{"version":"1.2.3","target":"{target}"}}'"#
            ),
        );
        assert_eq!(check(&good, Version(1, 2, 3)).await, Ok(()));
        assert!(
            check(&good, Version(1, 2, 4))
                .await
                .unwrap_err()
                .contains("not 1.2.4")
        );
        let other = script(
            dir.path(),
            "other",
            r#"echo '{"version":"1.2.3","target":"mips-unknown-linux-musl"}'"#,
        );
        assert!(
            check(&other, Version(1, 2, 3))
                .await
                .unwrap_err()
                .contains("mips")
        );
        let broken = script(dir.path(), "broken", "exit 3");
        assert!(
            check(&broken, Version(1, 2, 3))
                .await
                .unwrap_err()
                .contains("failed")
        );
        let garbage = script(dir.path(), "garbage", "echo hello");
        assert!(
            check(&garbage, Version(1, 2, 3))
                .await
                .unwrap_err()
                .contains("unreadable")
        );
        assert!(
            check(&dir.path().join("missing"), Version(1, 2, 3))
                .await
                .unwrap_err()
                .contains("can't run")
        );
    }

    #[test]
    fn a_restart_stops_serve_and_is_remembered() {
        let (tx, mut rx) = mpsc::channel(2);
        let restart = Restart::new(tx);
        assert!(!restart.asked());
        restart.ask();
        assert!(restart.asked());
        assert_eq!(rx.try_recv().unwrap(), "restarting into a new release");
    }
}
