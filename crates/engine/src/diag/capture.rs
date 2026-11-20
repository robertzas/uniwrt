//! Packet captures (features/observability.md, Diagnostics toolkit; D-142): tcpdump on a node
//! ([`uniwrt_node::diag::capture`]) whose `.pcap` goes to one reader as it comes, cut at the
//! query's size cap ([`PcapCut`]) and ended at its time cap. A capture starts once tcpdump has
//! written the file's header (it does at once, before any packet), so a device it doesn't know
//! or a filter it can't compile is refused before the download begins. Dropping the
//! [`Capture`] closes the channel, and the watchdog ends tcpdump. One capture per node, and
//! [`DiagConfig::captures`] in all.

use std::{
    sync::{Arc, Weak},
    time::Duration,
};

use bytes::Bytes;
use tokio::time::{self, Instant};
use uniwrt_core::NodeId;
use uniwrt_model::diag::{capture::CaptureQuery, tools::OptionalTool};
use uniwrt_node::{ExecStream, ExitStatus, Output, diag::capture::PcapCut};

use super::{Diagnostics, Inner};

/// How long tcpdump may take to start (SSH setup included).
const START_TIMEOUT: Duration = Duration::from_secs(15);
/// The most of tcpdump's stderr kept while it starts.
const MAX_STDERR: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CaptureError {
    /// The query can't run as asked.
    #[error("{0}")]
    Invalid(String),
    /// Too many captures run.
    #[error("{0}")]
    Busy(String),
    /// tcpdump isn't installed: the package that installs it.
    #[error("The device lacks tcpdump: install the {0} package")]
    Missing(&'static str),
    /// tcpdump refused the capture (a device it doesn't know, a filter it can't compile), in
    /// its words.
    #[error("tcpdump refused it: {0}")]
    Refused(String),
    /// The node isn't connected.
    #[error("The device isn't connected: check that it is online")]
    Offline,
    #[error("{0}")]
    Failed(String),
}

/// Why a capture ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureEnd {
    /// It reached its size cap.
    Size,
    /// It reached its time cap.
    Time,
    /// tcpdump exited (or the stream ended).
    Exited(ExitStatus),
    /// The link failed or fell behind: what happened.
    Lost(String),
}

/// A running capture: its `.pcap` bytes through [`Capture::next`].
#[derive(Debug)]
pub struct Capture {
    stream: ExecStream,
    cut: PcapCut,
    first: Option<Bytes>,
    deadline: Instant,
    end: Option<CaptureEnd>,
    _slot: Slot,
}

/// Holds a node's capture slot.
#[derive(Debug)]
struct Slot {
    inner: Weak<Inner>,
    node: NodeId,
}

impl Drop for Slot {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.upgrade() {
            let mut captures = inner.captures_lock();
            if let Some(at) = captures.iter().position(|node| *node == self.node) {
                captures.swap_remove(at);
            }
        }
    }
}

impl Inner {
    fn captures_lock(&self) -> std::sync::MutexGuard<'_, Vec<NodeId>> {
        self.captures
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Diagnostics {
    /// Starts capturing on `node` as `query` asks; answers once tcpdump runs.
    pub async fn capture(
        &self,
        node: NodeId,
        query: &CaptureQuery,
    ) -> Result<Capture, CaptureError> {
        query.validate().map_err(CaptureError::Invalid)?;
        let slot = self.capture_slot(node)?;
        let argv = uniwrt_node::diag::capture::command(query);
        let started = Instant::now();
        let limit = started + START_TIMEOUT;
        let late = || {
            CaptureError::Failed(format!(
                "tcpdump didn't start within {} s",
                START_TIMEOUT.as_secs()
            ))
        };
        let mut stream = time::timeout_at(limit, (self.inner.start)(node, argv))
            .await
            .map_err(|_| late())?
            .map_err(link_error)?;
        let mut stderr = Vec::new();
        let first = loop {
            match time::timeout_at(limit, stream.next())
                .await
                .map_err(|_| late())?
            {
                Some(Ok(Output::Stdout(data))) if !data.is_empty() => break data,
                Some(Ok(Output::Stdout(_))) => {}
                Some(Ok(Output::Stderr(data))) => {
                    let room = MAX_STDERR.saturating_sub(stderr.len());
                    stderr.extend_from_slice(&data[..room.min(data.len())]);
                }
                Some(Ok(Output::Exit(ExitStatus::Code(127)))) => {
                    return Err(CaptureError::Missing(OptionalTool::Tcpdump.package()));
                }
                Some(Ok(Output::Exit(status))) => {
                    return Err(CaptureError::Refused(refusal(&stderr, &status)));
                }
                Some(Err(err)) => return Err(link_error(err)),
                None => {
                    return Err(CaptureError::Failed(
                        "tcpdump ended without a word".to_owned(),
                    ));
                }
            }
        };
        let mut cut = PcapCut::new(query.max_bytes());
        let first = Bytes::from(cut.push(&first));
        Ok(Capture {
            stream,
            cut,
            first: Some(first),
            deadline: started + Duration::from_secs(u64::from(query.seconds)),
            end: None,
            _slot: slot,
        })
    }

    fn capture_slot(&self, node: NodeId) -> Result<Slot, CaptureError> {
        let mut captures = self.inner.captures_lock();
        if captures.contains(&node) {
            return Err(CaptureError::Busy(
                "This device captures already: wait for that capture to end, or cancel its download"
                    .to_owned(),
            ));
        }
        if captures.len() >= self.inner.config.captures {
            return Err(CaptureError::Busy(format!(
                "{} captures are running already: wait for one to end",
                captures.len()
            )));
        }
        captures.push(node);
        Ok(Slot {
            inner: Arc::downgrade(&self.inner),
            node,
        })
    }
}

impl Capture {
    /// The `.pcap`'s next bytes; `None` once it has ended ([`Self::end`] says why).
    pub async fn next(&mut self) -> Option<Bytes> {
        if let Some(first) = self.first.take()
            && !first.is_empty()
        {
            return Some(first);
        }
        while self.end.is_none() {
            if self.cut.done() {
                self.end = Some(CaptureEnd::Size);
                break;
            }
            tokio::select! {
                biased;
                () = time::sleep_until(self.deadline) => self.end = Some(CaptureEnd::Time),
                next = self.stream.next() => match next {
                    Some(Ok(Output::Stdout(data))) => {
                        let passed = self.cut.push(&data);
                        if !passed.is_empty() {
                            return Some(Bytes::from(passed));
                        }
                    }
                    Some(Ok(Output::Stderr(_))) => {}
                    Some(Ok(Output::Exit(status))) => self.end = Some(CaptureEnd::Exited(status)),
                    Some(Err(err)) => self.end = Some(CaptureEnd::Lost(err.to_string())),
                    None => self.end = Some(CaptureEnd::Exited(ExitStatus::Unknown)),
                },
            }
        }
        None
    }

    /// Why it ended, once it has.
    pub const fn end(&self) -> Option<&CaptureEnd> {
        self.end.as_ref()
    }

    /// The bytes passed so far.
    pub const fn bytes(&self) -> u64 {
        self.cut.bytes()
    }

    /// The packets passed so far.
    pub const fn packets(&self) -> u64 {
        self.cut.packets()
    }
}

fn link_error(err: uniwrt_node::Error) -> CaptureError {
    match err {
        uniwrt_node::Error::Disconnected { .. } => CaptureError::Offline,
        other => CaptureError::Failed(format!("Cannot start tcpdump: {other}")),
    }
}

/// tcpdump's own reason, from its stderr: its last `tcpdump: …` line that isn't a warning.
fn refusal(stderr: &[u8], status: &ExitStatus) -> String {
    let text = String::from_utf8_lossy(stderr);
    text.lines()
        .filter_map(|line| line.strip_prefix("tcpdump: "))
        .rfind(|line| !line.starts_with("WARNING"))
        .or_else(|| text.lines().map(str::trim).find(|line| !line.is_empty()))
        .map_or_else(
            || match status {
                ExitStatus::Code(code) => format!("it exited with {code}"),
                ExitStatus::Signal(signal) => format!("it ended by signal {signal}"),
                ExitStatus::Unknown => "it ended without an exit status".to_owned(),
            },
            str::to_owned,
        )
}

#[cfg(test)]
mod tests;
