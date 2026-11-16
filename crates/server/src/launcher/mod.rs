//! The launcher (D-121, D-123): `uniwrt serve` in a release build supervises the controller as
//! a child process, so the controller can upgrade itself in a container whose image it can't
//! change. It runs the newest verified binary staged in `<data>/bin` (or its own), forwards
//! SIGTERM and SIGINT, and passes the child's exit code on.
//!
//! - **Upgrades:** the controller stages a binary, names it the trial in `state.json` and exits
//!   with [`RESTART`]. The launcher copies the database to `bin/rollback/` and starts the
//!   trial; healthy within three minutes and still after a ten-second soak, it becomes the
//!   current binary. Otherwise it is stopped, the database put back as it was (a newer schema
//!   would lock the older binary out), the binary rejected for good, the failure kept for the
//!   notice, and the previous binary started again.
//! - **Image upgrades:** a staged binary runs only while newer than the image's; a newer image
//!   wins and the staged binaries are removed.
//! - **Installs** ([`Updater`], M7.1b2): the controller's side of an upgrade: the release
//!   staged and verified, run once, named the trial, then the restart request ([`Restart`]).
//!   A restart asked for while a stop signal came is a stop.
//! - **Not supervised:** a child (`UNIWRT_SUPERVISED` set), `--self-upgrade off`, demo mode and
//!   development builds (their version isn't `X.Y.Z`) serve directly.

mod child;
mod state;
#[cfg(test)]
mod tests;
mod update;

use std::{ffi::OsString, path::PathBuf, sync::Arc, time::SystemTime};

use anyhow::Context as _;
use rustix::process::Signal;
use tokio::{
    signal::unix::{SignalKind, signal},
    sync::mpsc,
};
use uniwrt_firmware::Version;

pub(crate) use child::SUPERVISED;
use child::{Gate, Probe, Timing};
pub(crate) use state::{Bin, State};
pub(crate) use update::{Restart, Updater};

use crate::{build_info, cli::ServeArgs, healthcheck, logging, serve};

/// The exit code with which the controller asks to be started again (a trial is pending).
pub(crate) const RESTART: i32 = 75;

/// `serve`, supervised when self-upgrades apply. Returns the process's exit code.
pub(crate) fn run(args: ServeArgs) -> anyhow::Result<u8> {
    let builtin = build_info::VERSION.parse::<Version>().ok();
    let supervised = std::env::var_os(SUPERVISED).is_some();
    match builtin {
        Some(builtin) if args.self_upgrade && !args.demo && !supervised => {
            logging::init(&args.log)?;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .context("cannot start the launcher's runtime")?;
            runtime.block_on(supervise(&args, builtin))
        }
        _ => serve::run(args),
    }
}

async fn supervise(args: &ServeArgs, builtin: Version) -> anyhow::Result<u8> {
    let data = std::path::absolute(&args.data_dir)?;
    let own = std::env::current_exe().context("cannot find the launcher's own binary")?;
    let probe_at = healthcheck::probe_addr(args.http_listen);
    let probe: Probe =
        Arc::new(move || healthcheck::check(probe_at, std::time::Duration::from_secs(3)).is_ok());
    let supervisor = Supervisor {
        bin: Bin::new(&data),
        builtin,
        own,
        args: std::env::args_os().skip(1).collect(),
        probe,
        timing: Timing::DEFAULT,
    };
    supervisor.run(forward_signals()?).await
}

/// SIGTERM and SIGINT, as they arrive.
fn forward_signals() -> anyhow::Result<mpsc::Receiver<Signal>> {
    let mut term = signal(SignalKind::terminate()).context("cannot handle SIGTERM")?;
    let mut int = signal(SignalKind::interrupt()).context("cannot handle SIGINT")?;
    let (tx, rx) = mpsc::channel(4);
    tokio::spawn(async move {
        loop {
            let signal = tokio::select! {
                Some(()) = term.recv() => Signal::TERM,
                Some(()) = int.recv() => Signal::INT,
                else => return,
            };
            if tx.send(signal).await.is_err() {
                return;
            }
        }
    });
    Ok(rx)
}

pub(crate) struct Supervisor {
    pub(crate) bin: Bin,
    /// The image's own version and binary.
    pub(crate) builtin: Version,
    pub(crate) own: PathBuf,
    /// The arguments every child gets: the launcher's own.
    pub(crate) args: Vec<OsString>,
    pub(crate) probe: Probe,
    pub(crate) timing: Timing,
}

impl Supervisor {
    pub(crate) async fn run(&self, mut signals: mpsc::Receiver<Signal>) -> anyhow::Result<u8> {
        loop {
            let mut state = self.bin.load();
            let choice = self.bin.choose(&mut state, self.builtin);
            if choice.trial && state.snapshot != state.trial {
                // Before the trial's first start only: a later start finds the database it
                // may already have migrated.
                self.bin
                    .snapshot()
                    .context("cannot copy the database before the trial")?;
                state.snapshot.clone_from(&state.trial);
            }
            self.save(&state);
            let program = choice.staged.as_deref().unwrap_or(&self.own);
            tracing::info!(version = %choice.version, trial = choice.trial, "starting the controller");
            let mut child = match child::spawn(program, &self.args) {
                Ok(child) => child,
                Err(err) if choice.staged.is_some() => {
                    self.failed(&mut state, choice.version, format!("it can't run: {err}"));
                    continue;
                }
                Err(err) => return Err(err).context("cannot start the controller"),
            };
            if choice.trial {
                match child::gate(&mut child, &self.probe, self.timing, &mut signals).await {
                    Gate::Healthy => {
                        tracing::info!(version = %choice.version, "the new controller is healthy");
                        state.confirm(choice.version);
                        self.bin.drop_snapshot();
                        self.save(&state);
                    }
                    Gate::Failed(reason) => {
                        child::stop(&mut child, self.timing.stop_grace).await;
                        self.failed(&mut state, choice.version, reason);
                        continue;
                    }
                    Gate::Stopped(status) => return Ok(child::exit_code(status)),
                }
            }
            let (status, signalled) = child::wait(&mut child, &mut signals).await;
            if status.code() != Some(RESTART) {
                return Ok(child::exit_code(status));
            }
            if signalled {
                // Asked to stop while the controller was restarting: stopping wins.
                return Ok(0);
            }
            tracing::info!("the controller asked to be started again");
        }
    }

    /// A trial failed: the database put back, the binary rejected, the failure kept.
    fn failed(&self, state: &mut State, version: Version, reason: String) {
        tracing::error!(%version, %reason, "the new controller failed its trial; going back");
        if state.snapshot.is_some()
            && let Err(err) = self.bin.restore()
        {
            tracing::error!(error = %err, "cannot put the database back as it was before the trial");
        }
        self.bin.drop_snapshot();
        let at = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |since| {
                i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
            });
        state.reject(version, reason, at);
        self.save(state);
    }

    fn save(&self, state: &State) {
        if let Err(err) = self.bin.save(state) {
            tracing::error!(error = %err, "cannot write the launcher's state");
        }
    }
}
