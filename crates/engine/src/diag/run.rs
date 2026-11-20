//! One run's task: starts the command, splits its output into lines and ends the run.

use std::{sync::Arc, time::Duration};

use tokio::{
    sync::oneshot,
    time::{self, Instant, MissedTickBehavior},
};
use uniwrt_core::NodeId;
use uniwrt_model::diag::{RunState, tools::OptionalTool};
use uniwrt_node::{ExecStream, ExitStatus, Output};

use super::Inner;

/// How often a run checks whether anybody still watches it.
const IDLE_CHECK: Duration = Duration::from_secs(5);

/// How a run ended: its state, exit code and why.
type End = (RunState, Option<u32>, Option<String>);

pub(super) async fn drive(
    inner: Arc<Inner>,
    id: u32,
    node: NodeId,
    argv: Vec<String>,
    limit: Duration,
    needs: Option<OptionalTool>,
    mut stop: oneshot::Receiver<()>,
) {
    let deadline = Instant::now() + limit;
    let (state, code, error) = tokio::select! {
        biased;
        _ = &mut stop => stopped(None),
        () = time::sleep_until(deadline) => at_limit(limit),
        started = (inner.start)(node, argv) => match started {
            Ok(stream) => pump(&inner, id, stream, deadline, limit, &mut stop)
                .await
                .missing(needs),
            Err(err) => (RunState::Failed, None, Some(worded(&err, "Cannot start it"))),
        },
    };
    inner.finish(id, state, code, error);
}

async fn pump(
    inner: &Inner,
    id: u32,
    mut stream: ExecStream,
    deadline: Instant,
    limit: Duration,
    stop: &mut oneshot::Receiver<()>,
) -> End {
    let max = inner.config.max_line;
    let (mut out, mut err) = (Splitter::new(max), Splitter::new(max));
    let mut check = time::interval_at(Instant::now() + IDLE_CHECK, IDLE_CHECK);
    check.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let end = loop {
        tokio::select! {
            biased;
            _ = &mut *stop => break stopped(None),
            () = time::sleep_until(deadline) => break at_limit(limit),
            _ = check.tick() => {
                if inner.idle(id) {
                    break stopped(Some("Stopped: nobody watched it for a minute".to_owned()));
                }
            }
            next = stream.next() => match next {
                Some(Ok(Output::Stdout(data))) => inner.add(id, out.push(&data), false),
                Some(Ok(Output::Stderr(data))) => inner.add(id, err.push(&data), true),
                Some(Ok(Output::Exit(status))) => break exited(&status),
                Some(Err(error)) => break (RunState::Failed, None, Some(worded(&error, "It failed"))),
                None => break exited(&ExitStatus::Unknown),
            },
        }
    };
    inner.add(id, out.finish().into_iter().collect(), false);
    inner.add(id, err.finish().into_iter().collect(), true);
    end
}

/// `err` for people: a lost link says so, other errors follow `what`.
fn worded(err: &uniwrt_node::Error, what: &str) -> String {
    match err {
        uniwrt_node::Error::Disconnected { .. } => {
            "The device isn't connected: check that it is online".to_owned()
        }
        other => format!("{what}: {other}"),
    }
}

/// Words an exit of 127 (the shell found no such command) as the package to install.
trait Missing {
    fn missing(self, needs: Option<OptionalTool>) -> Self;
}

impl Missing for End {
    fn missing(self, needs: Option<OptionalTool>) -> Self {
        match (self, needs) {
            ((RunState::Failed, Some(127), None), Some(tool)) => (
                RunState::Failed,
                Some(127),
                Some(format!(
                    "The device lacks the tool: install the {} package",
                    tool.package()
                )),
            ),
            (end, _) => end,
        }
    }
}

const fn stopped(why: Option<String>) -> End {
    (RunState::Stopped, None, why)
}

fn at_limit(limit: Duration) -> End {
    stopped(Some(format!(
        "Stopped at its limit of {} s",
        limit.as_secs()
    )))
}

fn exited(status: &ExitStatus) -> End {
    match status {
        ExitStatus::Code(0) => (RunState::Succeeded, Some(0), None),
        ExitStatus::Code(code) => (RunState::Failed, Some(*code), None),
        ExitStatus::Signal(signal) => (
            RunState::Failed,
            None,
            Some(format!("Ended by signal {signal}")),
        ),
        ExitStatus::Unknown => (
            RunState::Failed,
            None,
            Some("It ended without an exit status".to_owned()),
        ),
    }
}

/// Splits a stream's bytes into lines without their `\n` (or `\r\n`), cutting each at `max`
/// bytes (marked with `…`), as lossy UTF-8.
pub(super) struct Splitter {
    buf: Vec<u8>,
    max: usize,
    cut: bool,
}

impl Splitter {
    pub(super) const fn new(max: usize) -> Self {
        Self {
            buf: Vec::new(),
            max,
            cut: false,
        }
    }

    /// The lines that `data` completes.
    pub(super) fn push(&mut self, data: &[u8]) -> Vec<String> {
        let mut lines = Vec::new();
        for &byte in data {
            if byte == b'\n' {
                lines.push(self.take());
            } else if self.buf.len() < self.max {
                self.buf.push(byte);
            } else {
                self.cut = true;
            }
        }
        lines
    }

    /// The last line, if it didn't end with `\n`.
    pub(super) fn finish(&mut self) -> Option<String> {
        (!self.buf.is_empty() || self.cut).then(|| self.take())
    }

    fn take(&mut self) -> String {
        let line = self.buf.strip_suffix(b"\r").unwrap_or(&self.buf);
        let mut text = String::from_utf8_lossy(line).into_owned();
        if self.cut {
            text.push('…');
        }
        self.buf.clear();
        self.cut = false;
        text
    }
}
