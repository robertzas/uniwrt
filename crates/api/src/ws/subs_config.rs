//! The `config` topic of a connection (D-071): the site's latest revision, the revision each
//! node holds and drift (D-074), whole, when subscribed and again whenever any of them changes
//! (a save or a restore, [`Api::config_changed`]; an apply that ends; drift announced). Small, rare and read fresh, so it keeps
//! no state between flushes.

use tracing::warn;
use uniwrt_model::job::{Job, JobKind};
use uniwrt_store::{ApplyRepo as _, RevisionRepo as _};

use super::{message::ServerMessage, topic::BadTopic};
use crate::Api;

/// The topic's message, read fresh.
pub(super) async fn read(api: &Api, topic: String) -> Result<ServerMessage, BadTopic> {
    let store = api.store();
    let read = async {
        let head = store.revisions(None, 1).await?.into_iter().next();
        let applied = store.applied_revisions().await?;
        Ok::<_, uniwrt_store::Error>((head, applied))
    };
    let (head, applied) = read.await.map_err(|err| {
        warn!(%err, "cannot read the site's revisions for a WS topic");
        BadTopic(
            "The controller cannot read the site's revisions; its log has the details".to_owned(),
        )
    })?;
    Ok(ServerMessage::Config {
        topic,
        head,
        applied,
        drift: api.drifts().map(uniwrt_engine::Drifts::state),
    })
}

/// Whether `job`'s change can change what nodes hold: an apply that ended.
pub(crate) fn changes_config(job: &Job) -> bool {
    job.kind == JobKind::Apply && job.finished_at.is_some()
}
