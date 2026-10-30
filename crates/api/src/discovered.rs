//! Discovery, with a session: `GET /api/v1/discovered` reports the unadopted OpenWrt devices
//! that the adopted nodes see (features/adoption.md, Discovery), after waiting up to 3 seconds
//! for the probes of new ones. While the report is `scanning`, asking again finds more.

use std::time::Duration;

use axum::{Json, extract::State};
use uniwrt_model::discovery::DiscoveryReport;
use uniwrt_store::NodeRepo as _;

use crate::{Api, ApiResult, CurrentUser};

/// How long a request waits for probes.
const WAIT: Duration = Duration::from_secs(3);

pub(crate) async fn list(
    State(api): State<Api>,
    _: CurrentUser,
) -> ApiResult<Json<DiscoveryReport>> {
    let adopted: Vec<_> = api
        .store()
        .nodes()
        .await?
        .into_iter()
        .map(|node| {
            let facts = api.fleet().node(node.id).and_then(|view| view.facts);
            (node, facts)
        })
        .collect();
    Ok(Json(api.jobs().discover(&adopted, WAIT).await))
}
