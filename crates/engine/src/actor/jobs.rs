//! The requests an actor sends its agent, one at a time so a node that serves requests one at a
//! time is never asked twice at once: a queue of [`Job`]s, the one running, and its [`Outcome`].

use std::{collections::VecDeque, future::Future, pin::Pin, sync::Arc, time::Duration};

use uniwrt_node::{
    NodeLink, Result,
    proto::ops::{Collect, Collected, LogFollow, LogMode, Set, Subscribe, Topic},
};

use crate::schedule::Plan;

pub(super) enum Job {
    Subscribe(Vec<Topic>),
    Collect(Vec<Set>),
    /// `set_interval` for each interval of the plan.
    Schedule(Plan),
    Logs(LogFollow),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Subscribe,
    Collect,
    Schedule,
    Logs,
}

impl Job {
    fn kind(&self) -> Kind {
        match self {
            Self::Subscribe(_) => Kind::Subscribe,
            Self::Collect(_) => Kind::Collect,
            Self::Schedule(_) => Kind::Schedule,
            Self::Logs(_) => Kind::Logs,
        }
    }
}

/// How a job ended, for the agent of `generation`.
pub(super) struct Outcome {
    pub(super) generation: u64,
    pub(super) done: Done,
}

pub(super) enum Done {
    Subscribed(Result<()>),
    Collected(Result<Collected>),
    Scheduled { plan: Plan, result: Result<()> },
    Logs { mode: LogMode, result: Result<()> },
}

type Running = Pin<Box<dyn Future<Output = Outcome> + Send>>;

#[derive(Default)]
pub(super) struct Jobs {
    queue: VecDeque<Job>,
    running: Option<(Kind, Running)>,
}

impl Jobs {
    /// Queues a job. A collect joins the one waiting, if any; another kind replaces the waiting
    /// one of its kind, which it supersedes.
    pub(super) fn push(&mut self, job: Job) {
        let kind = job.kind();
        let Some(waiting) = self.queue.iter_mut().find(|queued| queued.kind() == kind) else {
            self.queue.push_back(job);
            return;
        };
        match (waiting, job) {
            (Job::Collect(sets), Job::Collect(more)) => {
                sets.extend(more);
                sets.sort_unstable();
                sets.dedup();
            }
            (waiting, job) => *waiting = job,
        }
    }

    /// A job of `kind` waits or runs.
    pub(super) fn pending(&self, kind: Kind) -> bool {
        self.running
            .as_ref()
            .is_some_and(|(running, _)| *running == kind)
            || self.queue.iter().any(|job| job.kind() == kind)
    }

    /// Drops the waiting jobs; the running one runs on.
    pub(super) fn clear(&mut self) {
        self.queue.clear();
    }

    /// Starts the next job unless one runs.
    pub(super) fn start<L: NodeLink>(&mut self, link: &Arc<L>, generation: u64, timeout: Duration) {
        if self.running.is_some() {
            return;
        }
        if let Some(job) = self.queue.pop_front() {
            self.running = Some((job.kind(), run(Arc::clone(link), generation, job, timeout)));
        }
    }

    /// The running job's outcome; pending while none runs. Cancel-safe: a job outlives a
    /// dropped call and is awaited again by the next.
    pub(super) async fn next(&mut self) -> Outcome {
        let Some((_, running)) = &mut self.running else {
            return std::future::pending().await;
        };
        let outcome = running.await;
        self.running = None;
        outcome
    }
}

fn run<L: NodeLink>(link: Arc<L>, generation: u64, job: Job, timeout: Duration) -> Running {
    Box::pin(async move {
        let done = match job {
            Job::Subscribe(topics) => {
                Done::Subscribed(link.request(&Subscribe { topics }, timeout).await.map(drop))
            }
            Job::Collect(sets) => Done::Collected(link.request(&Collect { sets }, timeout).await),
            Job::Schedule(plan) => {
                let mut result = Ok(());
                for request in plan.requests() {
                    if let Err(err) = link.request(&request, timeout).await {
                        result = Err(err);
                        break;
                    }
                }
                Done::Scheduled { plan, result }
            }
            Job::Logs(follow) => Done::Logs {
                mode: follow.mode,
                result: link.request(&follow, timeout).await.map(drop),
            },
        };
        Outcome { generation, done }
    })
}
