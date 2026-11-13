//! The apply an adoption on the gateway's adoption port ends with (features/adoption.md, Path C,
//! step 5; D-107): the revision the gateway holds, applied to the new node alone, as an apply job
//! of its own run in the adoption's task. On the LAN the node reaches the internet, so the
//! install step adds what its configuration needs (the full wpad, `kmod-batman-adv`, usteer),
//! and its own render brings the mesh and the WLANs. The gateway and the other nodes are left
//! as they are. The port path's Prepare step reads the same revision first ([`held`]).

use std::sync::Arc;

use tracing::{info, warn};
use uniwrt_core::{JobId, NodeId, RevisionId, UserId};
use uniwrt_model::{job::JobError, node::Node, site::revision::Revision};
use uniwrt_store::{ApplyRepo as _, RevisionRepo as _};

use crate::{Error, manage, runner::Shared};

/// How the node's apply ended.
pub(crate) struct Configured {
    pub(crate) job: JobId,
    pub(crate) revision: RevisionId,
    pub(crate) result: Result<(), JobError>,
}

/// The revision `gateway` holds (not unapplied edits); `Err` says why there is none.
pub(crate) async fn held(shared: &Shared, gateway: NodeId) -> Result<Revision, String> {
    let unread = |err: uniwrt_store::Error| {
        warn!(%err, "cannot read the gateway's revision");
        "Cannot read the revision the gateway holds".to_owned()
    };
    let held = shared
        .store
        .applied_revisions()
        .await
        .map_err(unread)?
        .into_iter()
        .find(|applied| applied.node == gateway)
        .ok_or_else(|| "The gateway holds no revision to apply".to_owned())?;
    shared
        .store
        .revision(held.revision)
        .await
        .map_err(unread)?
        .ok_or_else(|| format!("Revision {} is gone", held.revision))
}

/// Applies `revision` (the one the gateway holds, [`held`]) to `node` (managed, just adopted);
/// `Err` says why no apply started.
pub(crate) async fn adopted(
    shared: &Arc<Shared>,
    revision: &Revision,
    node: Node,
    by: Option<UserId>,
) -> Result<Configured, String> {
    let keys = vec![super::TARGET.to_owned(), manage::target(node.id)];
    let nodes = vec![node];
    let new = super::job(revision, &nodes, by);
    let (mut progress, _release) = shared.claim(keys, new).await.map_err(|err| match err {
        Error::Busy { .. } => "Another apply is running".to_owned(),
        err => {
            warn!(%err, "cannot start the node's apply");
            "Cannot start the node's apply".to_owned()
        }
    })?;
    let job = progress.job_id();
    let result = super::apply(shared, revision, nodes, &mut progress).await;
    match &result {
        Ok(()) => info!(%job, revision = %revision.meta.id, "applied to the adopted node"),
        Err(err) => warn!(%job, code = ?err.code, "{}", err.message),
    }
    progress.finish(result.clone().err()).await;
    Ok(Configured {
        job,
        revision: revision.meta.id,
        result,
    })
}
