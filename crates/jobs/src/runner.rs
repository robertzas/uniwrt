//! [`Jobs`]: starts jobs, one at a time per target, runs each in its own task, and keeps their
//! history in the store.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};

use tokio::{
    sync::{Semaphore, broadcast},
    task::AbortHandle,
};
use tracing::{Instrument as _, info, info_span, warn};
use uniwrt_core::{Clock, JobId, UserId};
use uniwrt_model::{
    discovery::DiscoveryReport,
    facts::NodeFacts,
    job::{Job, JobError, JobErrorCode, JobKind, NewJob},
    node::Node,
};
use uniwrt_node::{ControllerKey, HostKey, Target};
use uniwrt_store::{JobRepo as _, Store, Vault};

use crate::{
    AdoptRequest, BoxFuture, Dial, Error, Result,
    adopt::{self, STEPS},
    discovery::Discovery,
    progress::Progress,
};

/// How many job updates a slow subscriber may fall behind before it lags.
const UPDATES: usize = 64;
/// Adoptions running at once; more wait for a slot. Each holds a login and its backup (up to
/// 32 MiB) in memory.
const ADOPTIONS: usize = 4;

/// Starts watching nodes as they are adopted: the engine, as the server composes it.
pub trait Watch: Send + Sync + 'static {
    /// Watches a node just adopted, which shows `facts` until fresh ones arrive.
    fn watch(&self, node: Node, host_key: HostKey, facts: NodeFacts) -> BoxFuture<'_, ()>;
}

/// What [`Jobs::start`] needs.
pub struct JobsConfig {
    pub store: Store,
    pub vault: Vault,
    pub clock: Arc<dyn Clock>,
    /// The key that adopted nodes trust.
    pub key: ControllerKey,
    /// How to reach nodes; `None` where nothing can be adopted (some tests).
    pub dial: Option<Arc<dyn Dial>>,
    pub watch: Arc<dyn Watch>,
}

/// The job runner. Clones share it.
#[derive(Clone)]
pub struct Jobs {
    shared: Arc<Shared>,
}

pub(crate) struct Shared {
    pub(crate) store: Store,
    pub(crate) vault: Vault,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) key: ControllerKey,
    pub(crate) dial: Option<Arc<dyn Dial>>,
    pub(crate) watch: Arc<dyn Watch>,
    pub(crate) adoptions: Semaphore,
    discovery: Discovery,
    updates: broadcast::Sender<Arc<Job>>,
    running: Mutex<Running>,
}

/// The jobs running now and the targets they hold.
#[derive(Default)]
struct Running {
    tasks: BTreeMap<JobId, (String, AbortHandle)>,
    targets: BTreeSet<String>,
}

impl Shared {
    /// Announces a job's new state; nobody listening is fine.
    pub(crate) fn announce(&self, job: &Job) {
        let _ = self.updates.send(Arc::new(job.clone()));
    }

    /// Releases a finished job's target.
    pub(crate) fn finished(&self, id: JobId) {
        let mut running = self.running();
        if let Some((target, _)) = running.tasks.remove(&id) {
            running.targets.remove(&target);
        }
    }

    fn running(&self) -> MutexGuard<'_, Running> {
        self.running.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Jobs {
    /// Fails the jobs that a stop interrupted, then accepts new ones.
    pub async fn start(config: JobsConfig) -> Result<Self> {
        let interrupted = config.store.interrupt_jobs(interrupted()).await?;
        if interrupted > 0 {
            warn!(interrupted, "jobs were interrupted by the last stop");
        }
        Ok(Self {
            shared: Arc::new(Shared {
                store: config.store,
                vault: config.vault,
                discovery: Discovery::new(config.dial.clone(), Arc::clone(&config.clock)),
                clock: config.clock,
                key: config.key,
                dial: config.dial,
                watch: config.watch,
                adoptions: Semaphore::new(ADOPTIONS),
                updates: broadcast::channel(UPDATES).0,
                running: Mutex::default(),
            }),
        })
    }

    /// Whether nodes can be adopted here: not without a dial.
    pub fn can_adopt(&self) -> bool {
        self.shared.dial.is_some()
    }

    /// Reads a node's host key for the user to confirm (trust on first use).
    pub async fn probe(&self, target: &Target) -> Result<HostKey> {
        let dial = self.shared.dial.as_ref().ok_or(Error::Unavailable)?;
        Ok(dial.probe(target).await?)
    }

    /// Starts adopting a node, and returns the job as it starts; [`subscribe`](Self::subscribe)
    /// or [`job`](Self::job) follow it. One job at a time per target.
    pub async fn adopt(&self, request: AdoptRequest, by: Option<UserId>) -> Result<Job> {
        if self.shared.dial.is_none() {
            return Err(Error::Unavailable);
        }
        let target = request.target.to_string();
        let key = target.to_lowercase();
        if !self.shared.running().targets.insert(key.clone()) {
            return Err(Error::Busy { target });
        }
        let created = self
            .shared
            .store
            .create_job(NewJob {
                kind: JobKind::Adopt,
                target,
                created_by: by,
                steps: STEPS.to_vec(),
            })
            .await;
        let job = match created {
            Ok(job) => job,
            Err(err) => {
                self.shared.running().targets.remove(&key);
                return Err(err.into());
            }
        };
        info!(job_id = %job.id, target = %job.target, "adopting a node");
        self.shared.announce(&job);
        let progress = Progress::new(job.clone(), Arc::clone(&self.shared));
        let shared = Arc::clone(&self.shared);
        let span = info_span!("job", job_id = %job.id);
        // Locked while spawning, so that the task finds itself registered when it finishes.
        let release = Release(Arc::clone(&self.shared), job.id);
        let mut running = self.shared.running();
        let task = tokio::spawn(
            async move {
                let _release = release;
                adopt::run(shared, request, progress).await;
            }
            .instrument(span),
        );
        running.tasks.insert(job.id, (key, task.abort_handle()));
        Ok(job)
    }

    /// The OpenWrt devices that the `adopted` nodes (each with its latest facts) see and that
    /// are not adopted, after waiting up to `wait` for the probes of new ones.
    pub async fn discover(
        &self,
        adopted: &[(Node, Option<Arc<NodeFacts>>)],
        wait: Duration,
    ) -> DiscoveryReport {
        self.shared.discovery.scan(adopted, wait).await
    }

    pub async fn job(&self, id: JobId) -> Result<Option<Job>> {
        Ok(self.shared.store.job(id).await?)
    }

    /// The latest `limit` jobs, newest first.
    pub async fn jobs(&self, limit: usize) -> Result<Vec<Job>> {
        Ok(self.shared.store.jobs(limit).await?)
    }

    /// Every change to a job from now on, saved before it is announced.
    pub fn subscribe(&self) -> broadcast::Receiver<Arc<Job>> {
        self.shared.updates.subscribe()
    }

    /// Stops the running jobs and records them as interrupted.
    pub async fn shutdown(&self) {
        let tasks = std::mem::take(&mut self.shared.running().tasks);
        for (_, task) in tasks.values() {
            task.abort();
        }
        if !tasks.is_empty()
            && let Err(err) = self.shared.store.interrupt_jobs(interrupted()).await
        {
            warn!(%err, "cannot record the interrupted jobs");
        }
    }
}

/// Releases a job's target when its task ends, even by a panic.
struct Release(Arc<Shared>, JobId);

impl Drop for Release {
    fn drop(&mut self) {
        self.0.finished(self.1);
    }
}

fn interrupted() -> JobError {
    JobError {
        code: JobErrorCode::Interrupted,
        message: "The controller stopped while the job ran".to_owned(),
    }
}
