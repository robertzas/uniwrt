//! What the nightly optimizer works with: the store, the job runner, the engine, RF insight
//! and the API (to tell its WebSocket clients of a saved revision).

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use uniwrt_api::Api;
use uniwrt_core::{Clock, JobId, NodeId, RevisionId, Timestamp};
use uniwrt_engine::{Engine, Rf};
use uniwrt_jobs::Jobs;
use uniwrt_model::{
    events::{EventKind, NewEvent},
    job::JobState,
    node::NodeMode,
    rf::{NIGHTLY_HEAVY_KBPS, RfSettings},
    site::{
        SiteConfig,
        revision::{NewRevision, RevisionKind},
        validate::{has_errors, validate},
    },
    status::NodeState,
};
use uniwrt_rf::plan::Plan;
use uniwrt_store::{ApplyRepo as _, NodeRepo as _, RevisionRepo as _, Store};

use super::{ApplyError, Head, SaveError, Site};
use crate::fleet_link::FleetLink;

pub(super) struct Ctx {
    pub(super) store: Store,
    pub(super) jobs: Jobs,
    pub(super) engine: Engine<FleetLink>,
    pub(super) rf: Rf,
    pub(super) api: Api,
    pub(super) clock: Arc<dyn Clock>,
}

impl Site for Ctx {
    fn settings(&self) -> RfSettings {
        self.rf.view().settings
    }

    fn now(&self) -> Timestamp {
        self.clock.now()
    }

    async fn head(&self) -> Result<Option<Head>, String> {
        let head = self
            .store
            .head_revision()
            .await
            .map_err(|e| e.to_string())?;
        Ok(head.map(|head| Head {
            id: head.meta.id,
            config: head.config,
        }))
    }

    async fn nodes(
        &self,
    ) -> Result<(BTreeMap<NodeId, NodeMode>, BTreeMap<NodeId, RevisionId>), String> {
        let nodes = self.store.nodes().await.map_err(|e| e.to_string())?;
        let applied = self
            .store
            .applied_revisions()
            .await
            .map_err(|e| e.to_string())?;
        Ok((
            nodes.into_iter().map(|node| (node.id, node.mode)).collect(),
            applied.into_iter().map(|a| (a.node, a.revision)).collect(),
        ))
    }

    async fn plan(
        &self,
        config: SiteConfig,
        modes: BTreeMap<NodeId, NodeMode>,
    ) -> Result<(Plan, BTreeSet<(NodeId, String)>), String> {
        let plan = self
            .rf
            .plan(config, modes)
            .await
            .map_err(|e| e.to_string())?;
        let busy = self
            .rf
            .busy(NIGHTLY_HEAVY_KBPS)
            .await
            .map_err(|e| e.to_string())?;
        Ok((plan, busy))
    }

    fn offline(&self, node: NodeId) -> bool {
        self.engine
            .node(node)
            .is_none_or(|view| view.status.state == NodeState::Offline)
    }

    async fn job_running(&self) -> bool {
        self.jobs
            .jobs(20)
            .await
            .is_ok_and(|jobs| jobs.iter().any(|job| job.state == JobState::Running))
    }

    async fn save(
        &self,
        base: RevisionId,
        config: SiteConfig,
        message: String,
    ) -> Result<RevisionId, SaveError> {
        let nodes = self
            .store
            .nodes()
            .await
            .map_err(|e| SaveError::Failed(e.to_string()))?;
        let ids: BTreeSet<NodeId> = nodes.iter().map(|node| node.id).collect();
        if has_errors(&validate(&config, &ids)) {
            return Err(SaveError::Failed(
                "the planned site doesn't validate".to_owned(),
            ));
        }
        let new = NewRevision {
            base: Some(base),
            kind: RevisionKind::Edit,
            restored_from: None,
            author_id: None,
            author: None,
            message,
            config,
        };
        match self.store.commit_revision(new, Vec::new()).await {
            Ok(meta) => {
                self.api.config_changed();
                Ok(meta.id)
            }
            Err(uniwrt_store::Error::NotLatest { .. }) => Err(SaveError::NotLatest),
            Err(err) => Err(SaveError::Failed(err.to_string())),
        }
    }

    async fn apply(&self, revision: RevisionId) -> Result<JobId, ApplyError> {
        match self.jobs.apply(revision, None).await {
            Ok(job) => Ok(job.id),
            Err(uniwrt_jobs::Error::Busy { .. }) => Err(ApplyError::Busy),
            Err(err) => Err(ApplyError::Failed(err.to_string())),
        }
    }

    fn note(&self, kind: EventKind) {
        self.engine.note(NewEvent::new(self.clock.now(), kind));
    }
}
