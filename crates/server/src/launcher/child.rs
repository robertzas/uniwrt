//! The supervised controller: started with the launcher's arguments, signals forwarded to it,
//! its health watched during a trial, and stopped when the trial fails.

use std::{
    ffi::OsString, os::unix::process::ExitStatusExt as _, path::Path, process::ExitStatus,
    sync::Arc, time::Duration,
};

use rustix::process::{Pid, Signal, kill_process};
use tokio::{process::Child, sync::mpsc, time::Instant};

/// The variable that tells a controller it runs under the launcher (and may ask for a restart).
pub(crate) const SUPERVISED: &str = "UNIWRT_SUPERVISED";

/// A blocking probe of the controller's health (`GET /api/v1/health` on loopback).
pub(crate) type Probe = Arc<dyn Fn() -> bool + Send + Sync>;

/// How long a trial may take, and how its health is watched.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Timing {
    /// Until the first healthy answer (migrations run before the listeners open).
    pub(crate) trial: Duration,
    /// How long it must stay up and healthy after that.
    pub(crate) soak: Duration,
    pub(crate) probe_every: Duration,
    /// Between SIGTERM and SIGKILL when a failed trial is stopped.
    pub(crate) stop_grace: Duration,
}

impl Timing {
    pub(crate) const DEFAULT: Self = Self {
        trial: Duration::from_mins(3),
        soak: Duration::from_secs(10),
        probe_every: Duration::from_secs(1),
        stop_grace: Duration::from_secs(10),
    };
}

pub(crate) fn spawn(program: &Path, args: &[OsString]) -> std::io::Result<Child> {
    tokio::process::Command::new(program)
        .args(args)
        .env(SUPERVISED, "1")
        .kill_on_drop(true)
        .spawn()
}

/// Sends `signal` to the child, if it still runs.
pub(crate) fn forward(child: &Child, signal: Signal) {
    let pid = child
        .id()
        .and_then(|id| i32::try_from(id).ok())
        .and_then(Pid::from_raw);
    if let Some(pid) = pid
        && let Err(err) = kill_process(pid, signal)
    {
        tracing::warn!(error = %err, "cannot forward a signal to the controller");
    }
}

/// Waits for the child to exit, forwarding the signals the launcher receives.
/// Also tells whether a signal came (and was forwarded) meanwhile: then a restart request is
/// a stop.
pub(crate) async fn wait(
    child: &mut Child,
    signals: &mut mpsc::Receiver<Signal>,
) -> (ExitStatus, bool) {
    let mut signalled = false;
    loop {
        tokio::select! {
            status = child.wait() => {
                return (status.unwrap_or_else(|_| ExitStatus::from_raw(1 << 8)), signalled);
            }
            Some(signal) = signals.recv() => {
                signalled = true;
                forward(child, signal);
            }
        }
    }
}

/// How a trial ended.
#[derive(Debug)]
pub(crate) enum Gate {
    /// Healthy, and still healthy after the soak.
    Healthy,
    /// Exited or never healthy: the reason, for the notice.
    Failed(String),
    /// A signal came during the trial: forwarded, and the child exited with this status.
    Stopped(ExitStatus),
}

/// Watches a trial: the child must answer healthy within `timing.trial` and still after
/// `timing.soak`, without exiting.
pub(crate) async fn gate(
    child: &mut Child,
    probe: &Probe,
    timing: Timing,
    signals: &mut mpsc::Receiver<Signal>,
) -> Gate {
    let deadline = Instant::now() + timing.trial;
    let mut healthy_since: Option<Instant> = None;
    loop {
        tokio::select! {
            status = child.wait() => {
                let status = status.unwrap_or_else(|_| ExitStatus::from_raw(1 << 8));
                return Gate::Failed(format!("it exited ({}) before it was healthy", describe(status)));
            }
            Some(signal) = signals.recv() => {
                forward(child, signal);
                return Gate::Stopped(wait(child, signals).await.0);
            }
            () = tokio::time::sleep(timing.probe_every) => {}
        }
        let probe = Arc::clone(probe);
        let healthy = tokio::task::spawn_blocking(move || probe())
            .await
            .unwrap_or(false);
        let now = Instant::now();
        match (healthy, healthy_since) {
            (true, Some(since)) if now.duration_since(since) >= timing.soak => {
                return Gate::Healthy;
            }
            (true, None) => healthy_since = Some(now),
            (false, Some(_)) => {
                return Gate::Failed("it stopped answering its health check".to_owned());
            }
            (false, None) if now >= deadline => {
                return Gate::Failed(format!(
                    "it wasn't healthy within {} s",
                    timing.trial.as_secs()
                ));
            }
            (true, Some(_)) | (false, None) => {}
        }
    }
}

/// Stops a child: SIGTERM, then SIGKILL after `grace`.
pub(crate) async fn stop(child: &mut Child, grace: Duration) {
    forward(child, Signal::TERM);
    if tokio::time::timeout(grace, child.wait()).await.is_err() {
        let _ = child.kill().await;
    }
}

/// The exit code the launcher passes on: the child's, or 128 + the signal that ended it.
pub(crate) fn exit_code(status: ExitStatus) -> u8 {
    match (status.code(), status.signal()) {
        (Some(code), _) => u8::try_from(code).unwrap_or(1),
        (None, Some(signal)) => u8::try_from(128 + signal).unwrap_or(1),
        (None, None) => 1,
    }
}

pub(crate) fn describe(status: ExitStatus) -> String {
    match (status.code(), status.signal()) {
        (Some(code), _) => format!("status {code}"),
        (None, Some(signal)) => format!("signal {signal}"),
        (None, None) => "unknown status".to_owned(),
    }
}
