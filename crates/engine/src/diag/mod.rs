//! The diagnostics toolkit's runs (features/observability.md, Diagnostics toolkit; D-141): a
//! tool started on a node over an exec stream ([`uniwrt_node::diag`]), its output split into
//! lines and kept in memory (the last [`DiagConfig::output_budget`] bytes), each batch announced
//! as a [`DiagUpdate`] to the WS hub. A run ends when the tool exits, when an admin stops it,
//! at its time limit, or once nobody has watched it ([`Diagnostics::watch`]) for
//! [`DiagConfig::idle`]; ended runs stay for [`DiagConfig::keep`]. Nothing is stored: a
//! restart forgets every run.

mod run;
#[cfg(test)]
mod tests;

use std::{
    collections::{BTreeMap, VecDeque},
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex, MutexGuard, PoisonError, Weak},
    time::Duration,
};

use tokio::{
    sync::{broadcast, oneshot},
    time::Instant,
};
use uniwrt_core::{Clock, NodeId};
use uniwrt_model::diag::{DiagEnd, DiagLine, DiagRun, DiagTool, RunState};
use uniwrt_node::ExecStream;

/// Starts a command on a node: [`Fleet::stream`](crate::Fleet::stream), or a test's fake.
pub type Starter = Arc<
    dyn Fn(
            NodeId,
            Vec<String>,
        ) -> Pin<Box<dyn Future<Output = uniwrt_node::Result<ExecStream>> + Send>>
        + Send
        + Sync,
>;

/// The runs' bounds.
#[derive(Debug, Clone)]
pub struct DiagConfig {
    /// The most runs running on one node.
    pub per_node: usize,
    /// The most runs running in all.
    pub total: usize,
    /// The output kept per run, in bytes of text; older lines go first.
    pub output_budget: usize,
    /// Longer lines are cut, in bytes.
    pub max_line: usize,
    /// A run nobody watches for this long is stopped.
    pub idle: Duration,
    /// How long an ended run stays.
    pub keep: Duration,
    /// The most ended runs kept; the oldest go first.
    pub kept: usize,
}

impl Default for DiagConfig {
    fn default() -> Self {
        Self {
            per_node: 2,
            total: 8,
            output_budget: 256 << 10,
            max_line: 4096,
            idle: Duration::from_mins(1),
            keep: Duration::from_mins(15),
            kept: 32,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DiagError {
    /// The tool can't run as asked.
    #[error("{0}")]
    Invalid(String),
    /// Too many runs are running.
    #[error("{0}")]
    Busy(String),
    #[error("no such run")]
    NotFound,
}

/// What a run added since the previous update: new lines, and how it ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagUpdate {
    pub id: u32,
    /// The index of the first of `lines` among every line the run produced, dropped ones
    /// included: a reader that has the lines before it skips none, and one that has more skips
    /// those it has.
    pub first: u64,
    pub lines: Vec<DiagLine>,
    /// The lines dropped from the run's start so far.
    pub dropped: u32,
    pub end: Option<DiagEnd>,
}

/// The runs. Clones share them.
#[derive(Clone)]
pub struct Diagnostics {
    inner: Arc<Inner>,
}

struct Inner {
    start: Starter,
    clock: Arc<dyn Clock>,
    config: DiagConfig,
    runs: Mutex<Runs>,
    updates: broadcast::Sender<Arc<DiagUpdate>>,
}

#[derive(Default)]
struct Runs {
    next: u32,
    slots: BTreeMap<u32, Slot>,
}

struct Slot {
    /// Without its lines, which are kept here.
    run: DiagRun,
    lines: VecDeque<DiagLine>,
    bytes: usize,
    watchers: usize,
    /// Since when nobody watches it (its start, at first).
    unwatched: Instant,
    ended: Option<Instant>,
    /// Stops it while it runs.
    stop: Option<oneshot::Sender<()>>,
}

impl Slot {
    fn snapshot(&self) -> DiagRun {
        DiagRun {
            lines: self.lines.iter().cloned().collect(),
            ..self.run.clone()
        }
    }
}

/// Keeps a run watched while held ([`Diagnostics::watch`]).
pub struct Watch {
    inner: Weak<Inner>,
    id: u32,
}

impl Drop for Watch {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.upgrade()
            && let Some(slot) = inner.lock().slots.get_mut(&self.id)
        {
            slot.watchers = slot.watchers.saturating_sub(1);
            if slot.watchers == 0 {
                slot.unwatched = Instant::now();
            }
        }
    }
}

impl Diagnostics {
    pub fn new(start: Starter, clock: Arc<dyn Clock>, config: DiagConfig) -> Self {
        Self {
            inner: Arc::new(Inner {
                start,
                clock,
                config,
                runs: Mutex::default(),
                updates: broadcast::Sender::new(256),
            }),
        }
    }

    /// Every run's updates. A receiver that falls behind skips ahead (`Lagged`) and should
    /// read the runs it follows again.
    pub fn subscribe(&self) -> broadcast::Receiver<Arc<DiagUpdate>> {
        self.inner.updates.subscribe()
    }

    /// Starts `tool` on `node` for `by`; answers the run as it starts.
    pub fn start(
        &self,
        node: NodeId,
        tool: DiagTool,
        by: Option<String>,
    ) -> Result<DiagRun, DiagError> {
        tool.validate().map_err(DiagError::Invalid)?;
        let config = &self.inner.config;
        let argv = uniwrt_node::diag::command(&tool);
        let limit = uniwrt_node::diag::limit(&tool);
        let needs = tool.needs();
        let (stop, stopped) = oneshot::channel();
        let run = {
            let mut runs = self.inner.lock();
            runs.prune(config);
            let running: Vec<NodeId> = runs
                .slots
                .values()
                .filter(|slot| slot.run.state == RunState::Running)
                .map(|slot| slot.run.node)
                .collect();
            if running.iter().filter(|n| **n == node).count() >= config.per_node {
                return Err(DiagError::Busy(format!(
                    "This device runs {} tools already: wait for one to end, or stop it",
                    config.per_node
                )));
            }
            if running.len() >= config.total {
                return Err(DiagError::Busy(format!(
                    "{} tools are running already: wait for one to end, or stop it",
                    config.total
                )));
            }
            runs.next += 1;
            let id = runs.next;
            let run = DiagRun {
                id,
                node,
                tool,
                state: RunState::Running,
                by,
                started_at: self.inner.clock.now(),
                ended_at: None,
                exit_code: None,
                error: None,
                lines: Vec::new(),
                dropped: 0,
            };
            runs.slots.insert(
                id,
                Slot {
                    run: run.clone(),
                    lines: VecDeque::new(),
                    bytes: 0,
                    watchers: 0,
                    unwatched: Instant::now(),
                    ended: None,
                    stop: Some(stop),
                },
            );
            run
        };
        tokio::spawn(run::drive(
            Arc::clone(&self.inner),
            run.id,
            node,
            argv,
            limit,
            needs,
            stopped,
        ));
        Ok(run)
    }

    /// The run `id` with its lines.
    pub fn get(&self, id: u32) -> Option<DiagRun> {
        let mut runs = self.inner.lock();
        runs.prune(&self.inner.config);
        runs.slots.get(&id).map(Slot::snapshot)
    }

    /// Every run kept, newest first, without their lines.
    pub fn list(&self) -> Vec<DiagRun> {
        let mut runs = self.inner.lock();
        runs.prune(&self.inner.config);
        runs.slots
            .values()
            .rev()
            .map(|slot| slot.run.clone())
            .collect()
    }

    /// The run `id` with its lines, watched while the guard is held: a run nobody watches is
    /// stopped after [`DiagConfig::idle`]. Subscribe ([`Self::subscribe`]) first, so that no
    /// update falls between.
    pub fn watch(&self, id: u32) -> Option<(DiagRun, Watch)> {
        let mut runs = self.inner.lock();
        runs.prune(&self.inner.config);
        let slot = runs.slots.get_mut(&id)?;
        slot.watchers += 1;
        let watch = Watch {
            inner: Arc::downgrade(&self.inner),
            id,
        };
        Some((slot.snapshot(), watch))
    }

    /// Stops the run `id` if it runs; answers it as it is (it ends shortly).
    pub fn stop(&self, id: u32) -> Result<DiagRun, DiagError> {
        let mut runs = self.inner.lock();
        let slot = runs.slots.get_mut(&id).ok_or(DiagError::NotFound)?;
        if let Some(stop) = slot.stop.take() {
            let _ = stop.send(());
        }
        Ok(slot.run.clone())
    }
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, Runs> {
        self.runs.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether nobody has watched the run `id` for [`DiagConfig::idle`].
    fn idle(&self, id: u32) -> bool {
        self.lock()
            .slots
            .get(&id)
            .is_some_and(|slot| slot.watchers == 0 && slot.unwatched.elapsed() >= self.config.idle)
    }

    /// Adds lines to the run `id`, dropping the oldest beyond the budget, and announces them.
    fn add(&self, id: u32, texts: Vec<String>, err: bool) {
        if texts.is_empty() {
            return;
        }
        let at = self.clock.now();
        let mut runs = self.lock();
        let Some(slot) = runs.slots.get_mut(&id) else {
            return;
        };
        let added = texts.len();
        for text in texts {
            slot.bytes += text.len();
            slot.lines.push_back(DiagLine { at, err, text });
        }
        while slot.bytes > self.config.output_budget
            && let Some(line) = slot.lines.pop_front()
        {
            slot.bytes -= line.text.len();
            slot.run.dropped = slot.run.dropped.saturating_add(1);
        }
        let kept = added.min(slot.lines.len());
        let update = DiagUpdate {
            id,
            first: produced(slot) - kept as u64,
            lines: slot
                .lines
                .range(slot.lines.len() - kept..)
                .cloned()
                .collect(),
            dropped: slot.run.dropped,
            end: None,
        };
        let _ = self.updates.send(Arc::new(update));
    }

    /// Ends the run `id` and announces it.
    fn finish(&self, id: u32, state: RunState, exit_code: Option<u32>, error: Option<String>) {
        let end = DiagEnd {
            state,
            ended_at: self.clock.now(),
            exit_code,
            error,
        };
        let mut runs = self.lock();
        let Some(slot) = runs.slots.get_mut(&id) else {
            return;
        };
        slot.run.state = end.state;
        slot.run.ended_at = Some(end.ended_at);
        slot.run.exit_code = end.exit_code;
        slot.run.error.clone_from(&end.error);
        slot.ended = Some(Instant::now());
        slot.stop = None;
        let update = DiagUpdate {
            id,
            first: produced(slot),
            lines: Vec::new(),
            dropped: slot.run.dropped,
            end: Some(end),
        };
        let _ = self.updates.send(Arc::new(update));
    }
}

/// How many lines the run produced, dropped ones included.
fn produced(slot: &Slot) -> u64 {
    u64::from(slot.run.dropped) + slot.lines.len() as u64
}

/// How many lines `run` produced, dropped ones included: the index of the next.
pub fn produced_by(run: &DiagRun) -> u64 {
    u64::from(run.dropped) + run.lines.len() as u64
}

impl Runs {
    /// Forgets runs that ended more than [`DiagConfig::keep`] ago, and the oldest ended ones
    /// beyond [`DiagConfig::kept`].
    fn prune(&mut self, config: &DiagConfig) {
        self.slots
            .retain(|_, slot| slot.ended.is_none_or(|ended| ended.elapsed() < config.keep));
        let ended: Vec<u32> = self
            .slots
            .iter()
            .filter(|(_, slot)| slot.ended.is_some())
            .map(|(id, _)| *id)
            .collect();
        for id in ended.iter().take(ended.len().saturating_sub(config.kept)) {
            self.slots.remove(id);
        }
    }
}
