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
use uniwrt_core::{Clock, JobId, NodeId, RevisionId, UserId};
use uniwrt_model::{
    apply::ApplyPlan,
    discovery::DiscoveryReport,
    facts::NodeFacts,
    job::{Job, JobError, JobErrorCode, JobKind, NewJob},
    node::Node,
};
use uniwrt_node::{ControllerKey, HostKey, Target};
use uniwrt_store::{JobRepo as _, NodeRepo as _, RevisionRepo as _, Store, Vault};

use crate::{
    AdoptRequest, BoxFuture, Dial, Error, ImportDraft, Result,
    adopt::{self, STEPS},
    apply,
    discovery::Discovery,
    manage,
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

    /// Watches a node at its new address (`node.host`), which an apply moved it to.
    fn moved(&self, node: Node, host_key: HostKey) -> BoxFuture<'_, ()>;
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
    pub(crate) timing: apply::ApplyTiming,
    discovery: Discovery,
    updates: broadcast::Sender<Arc<Job>>,
    running: Mutex<Running>,
}

/// The jobs running now and the targets they hold.
#[derive(Default)]
struct Running {
    tasks: BTreeMap<JobId, (Vec<String>, AbortHandle)>,
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
        if let Some((keys, _)) = running.tasks.remove(&id) {
            for key in keys {
                running.targets.remove(&key);
            }
        }
    }

    fn running(&self) -> MutexGuard<'_, Running> {
        self.running.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Jobs {
    /// Fails the jobs that a stop interrupted, then accepts new ones.
    pub async fn start(config: JobsConfig) -> Result<Self> {
        Self::start_with(config, apply::ApplyTiming::default()).await
    }

    /// [`start`](Self::start) with the apply's `timing`, for tests that can't wait
    /// production's timers.
    pub async fn start_with(config: JobsConfig, timing: apply::ApplyTiming) -> Result<Self> {
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
                timing,
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
        let job = NewJob {
            kind: JobKind::Adopt,
            target,
            node: None,
            revision: None,
            created_by: by,
            steps: STEPS.to_vec(),
            nodes: Vec::new(),
        };
        self.spawn(vec![key], job, |shared, progress| {
            adopt::run(shared, request, progress)
        })
        .await
    }

    /// Starts enabling management on a node (`enable`), or turning it off, and returns the job
    /// as it starts. One job at a time per node.
    pub async fn manage(&self, node: NodeId, enable: bool, by: Option<UserId>) -> Result<Job> {
        if self.shared.dial.is_none() {
            return Err(Error::Unavailable);
        }
        let node = self
            .shared
            .store
            .node(node)
            .await?
            .ok_or(Error::NoNode(node))?;
        let job = manage::job(&node, enable, by);
        self.spawn(
            vec![manage::target(node.id)],
            job,
            move |shared, progress| manage::run(shared, node, enable, progress),
        )
        .await
    }

    /// The plan of applying the site's latest revision now: what each node would get, read
    /// fresh from the nodes. Nothing changes on them.
    pub async fn plan(&self) -> Result<ApplyPlan> {
        if self.shared.dial.is_none() {
            return Err(Error::Unavailable);
        }
        let revision = self
            .shared
            .store
            .head_revision()
            .await?
            .ok_or(Error::NoRevision)?;
        let nodes = self.shared.store.nodes().await?;
        apply::plan(&self.shared, &revision, nodes)
            .await
            .map_err(|err| Error::Failed(err.message))
    }

    /// The draft of importing what every adopted node holds into the site's latest revision
    /// (brownfield import, D-072): read fresh from the nodes, its secrets sealed, for the
    /// caller to show and commit. Nothing changes on the nodes; not while an apply runs.
    pub async fn import(&self) -> Result<ImportDraft> {
        if self.shared.dial.is_none() {
            return Err(Error::Unavailable);
        }
        if self.shared.running().targets.contains(apply::TARGET) {
            return Err(Error::Busy {
                target: "the configuration".to_owned(),
            });
        }
        let head = self
            .shared
            .store
            .head_revision()
            .await?
            .ok_or(Error::NoRevision)?;
        let nodes = self.shared.store.nodes().await?;
        crate::import::draft(&self.shared, &head, nodes).await
    }

    /// Starts applying the site's latest revision, which must be `revision` (the one the user
    /// reviewed), to every managed node, and returns the job as it starts. One apply at a
    /// time, and none while another job runs on a managed node.
    pub async fn apply(&self, revision: RevisionId, by: Option<UserId>) -> Result<Job> {
        if self.shared.dial.is_none() {
            return Err(Error::Unavailable);
        }
        let head = self
            .shared
            .store
            .head_revision()
            .await?
            .ok_or(Error::NoRevision)?;
        if head.meta.id != revision {
            return Err(Error::Stale {
                revision,
                head: head.meta.id,
            });
        }
        let nodes = self.shared.store.nodes().await?;
        let job = apply::job(&head, &nodes, by);
        let keys = std::iter::once(apply::TARGET.to_owned())
            .chain(
                nodes
                    .iter()
                    .filter(|node| node.mode == uniwrt_model::node::NodeMode::Managed)
                    .map(|node| manage::target(node.id)),
            )
            .collect();
        self.spawn(keys, job, move |shared, progress| {
            apply::run(shared, head, nodes, progress)
        })
        .await
    }

    /// Records a new job and runs it in its own task, holding `keys` (its targets) meanwhile.
    async fn spawn<F, Fut>(&self, keys: Vec<String>, new: NewJob, run: F) -> Result<Job>
    where
        F: FnOnce(Arc<Shared>, Progress) -> Fut,
        Fut: Future<Output = ()> + Send + 'static,
    {
        {
            let mut running = self.shared.running();
            if keys.iter().any(|key| running.targets.contains(key)) {
                return Err(Error::Busy { target: new.target });
            }
            running.targets.extend(keys.iter().cloned());
        }
        let job = match self.shared.store.create_job(new).await {
            Ok(job) => job,
            Err(err) => {
                let mut running = self.shared.running();
                for key in &keys {
                    running.targets.remove(key);
                }
                return Err(err.into());
            }
        };
        info!(job_id = %job.id, kind = ?job.kind, target = %job.target, "starting a job");
        self.shared.announce(&job);
        let progress = Progress::new(job.clone(), Arc::clone(&self.shared));
        let work = run(Arc::clone(&self.shared), progress);
        let span = info_span!("job", job_id = %job.id);
        // Locked while spawning, so that the task finds itself registered when it finishes.
        let release = Release(Arc::clone(&self.shared), job.id);
        let mut running = self.shared.running();
        let task = tokio::spawn(
            async move {
                let _release = release;
                work.await;
            }
            .instrument(span),
        );
        running.tasks.insert(job.id, (keys, task.abort_handle()));
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
