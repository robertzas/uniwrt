//! The `diag:<id>` topic of a connection (D-141): one diagnostics run, its lines kept by the
//! controller as a snapshot, then the lines it adds and how it ends. The topic watches the run
//! (an unwatched run is stopped after a minute).

use std::collections::VecDeque;

use uniwrt_engine::{DiagUpdate, Watch, produced_by};
use uniwrt_model::diag::{DiagEnd, DiagLine, DiagRun, RunState};

use super::message::ServerMessage;

/// The most lines a `diag` topic keeps between flushes; older ones are dropped.
const MAX_LINES: usize = 2_000;

pub(super) struct DiagPending {
    id: u32,
    /// The index of the next line the client lacks (every line produced before it was sent or
    /// dropped).
    next: u64,
    lines: VecDeque<DiagLine>,
    dropped: u32,
    end: Option<DiagEnd>,
    /// Whether the client knows how it ended.
    ended: bool,
    dirty: bool,
    _watch: Watch,
}

impl DiagPending {
    /// The topic's state after its snapshot `run`.
    pub(super) fn new(run: &DiagRun, watch: Watch) -> Self {
        Self {
            id: run.id,
            next: produced_by(run),
            lines: VecDeque::new(),
            dropped: run.dropped,
            end: None,
            ended: run.state != RunState::Running,
            dirty: false,
            _watch: watch,
        }
    }

    /// Notes what `update` adds, if it is about this run and the client lacks it.
    pub(super) fn apply(&mut self, update: &DiagUpdate) {
        if update.id != self.id {
            return;
        }
        let skip = self.next.saturating_sub(update.first);
        let fresh = update
            .lines
            .iter()
            .skip(usize::try_from(skip).unwrap_or(usize::MAX));
        for line in fresh {
            if self.lines.len() == MAX_LINES {
                self.lines.pop_front();
            }
            self.lines.push_back(line.clone());
            self.next += 1;
            self.dirty = true;
        }
        if update.dropped != self.dropped {
            self.dropped = update.dropped;
            self.dirty = true;
        }
        if let Some(end) = &update.end
            && !self.ended
        {
            self.end = Some(end.clone());
            self.ended = true;
            self.dirty = true;
        }
    }

    pub(super) fn flush(&mut self, topic: String) -> Option<ServerMessage> {
        if !std::mem::take(&mut self.dirty) {
            return None;
        }
        Some(ServerMessage::DiagDelta {
            topic,
            lines: self.lines.drain(..).collect(),
            dropped: self.dropped,
            end: self.end.take(),
        })
    }
}
