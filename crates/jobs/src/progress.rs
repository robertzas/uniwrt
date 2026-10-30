//! [`Progress`]: a running job's record, saved and announced at each change of a step.

use std::sync::Arc;

use tracing::warn;
use uniwrt_core::NodeId;
use uniwrt_model::job::{Job, JobError, JobState, StepName, StepState};
use uniwrt_store::JobRepo as _;

use crate::runner::Shared;

pub(crate) struct Progress {
    job: Job,
    shared: Arc<Shared>,
}

impl Progress {
    pub(crate) fn new(job: Job, shared: Arc<Shared>) -> Self {
        Self { job, shared }
    }

    /// Starts a pending step.
    pub(crate) async fn start(&mut self, name: StepName) {
        let now = self.shared.clock.now();
        if let Some(step) = self.step(name) {
            step.state = StepState::Running;
            step.started_at = Some(now);
        }
        self.save().await;
    }

    /// Finishes the running step.
    pub(crate) async fn done(&mut self, detail: Option<String>) {
        self.finish_running(StepState::Done, detail);
        self.save().await;
    }

    /// Skips a step: the running one, or a pending one.
    pub(crate) async fn skip(&mut self, name: StepName, detail: impl Into<String>) {
        let now = self.shared.clock.now();
        if let Some(step) = self.step(name) {
            step.state = StepState::Skipped;
            step.detail = Some(detail.into());
            step.started_at.get_or_insert(now);
            step.finished_at = Some(now);
        }
        self.save().await;
    }

    pub(crate) fn set_node(&mut self, node: NodeId) {
        self.job.node = Some(node);
    }

    /// Ends the job: failed with `error` (and its running step with it), or succeeded.
    pub(crate) async fn finish(mut self, error: Option<JobError>) {
        self.job.state = if error.is_some() {
            JobState::Failed
        } else {
            JobState::Succeeded
        };
        self.finish_running(StepState::Failed, None);
        self.job.error = error;
        self.job.finished_at = Some(self.shared.clock.now());
        self.save().await;
        self.shared.finished(self.job.id);
    }

    fn step(&mut self, name: StepName) -> Option<&mut uniwrt_model::job::Step> {
        self.job.steps.iter_mut().find(|step| step.name == name)
    }

    fn finish_running(&mut self, state: StepState, detail: Option<String>) {
        let now = self.shared.clock.now();
        if let Some(step) = self
            .job
            .steps
            .iter_mut()
            .find(|step| step.state == StepState::Running)
        {
            step.state = state;
            step.finished_at = Some(now);
            if detail.is_some() {
                step.detail = detail;
            }
        }
    }

    /// Saves the job, then announces it: a reader who hears of a change finds it stored.
    async fn save(&mut self) {
        if let Err(err) = self.shared.store.update_job(&self.job).await {
            warn!(job_id = %self.job.id, %err, "cannot save the job's progress");
        }
        self.shared.announce(&self.job);
    }
}
