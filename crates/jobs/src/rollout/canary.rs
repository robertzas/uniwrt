//! The canary's check at the end of its soak (D-125): it stayed up (no `node_offline` or
//! `node_rebooted` event since its upgrade ended), it still accepts the controller's login with
//! its pinned host key, and it runs the release it was upgraded to.

use uniwrt_core::{NodeId, Timestamp};
use uniwrt_model::events::EventFilter;
use uniwrt_node::proto::ops::Set;
use uniwrt_store::{EventRepo as _, NodeRepo as _};

use crate::{apply::survey, manage, runner::Shared};

/// Why the canary isn't fit to go on with, for people; `Ok` with what was found when it is.
pub(crate) async fn check(
    shared: &Shared,
    node: NodeId,
    since: Timestamp,
    release: Option<&str>,
) -> Result<String, String> {
    let filter = EventFilter {
        kinds: vec!["node_offline".to_owned(), "node_rebooted".to_owned()],
        nodes: vec![node],
        from: Some(since),
        ..EventFilter::default()
    };
    let events = shared
        .store
        .events(filter, None, 1)
        .await
        .map_err(|err| format!("Cannot read its events: {err}"))?;
    if let Some(event) = events.first() {
        let what = if event.what.name() == "node_offline" {
            "went offline"
        } else {
            "rebooted by itself"
        };
        return Err(format!("It {what} during its soak"));
    }
    let found = shared
        .store
        .node(node)
        .await
        .map_err(|err| format!("Cannot read the node: {err}"))?
        .ok_or_else(|| "It was removed from the controller".to_owned())?;
    let remote = manage::login(shared, &found)
        .await
        .map_err(|err| format!("It can't be reached: {}", err.message))?;
    let facts = survey::collect(shared, remote.as_ref(), &[Set::Board], &[Set::Board]).await;
    remote.close().await;
    let facts = facts.map_err(|why| format!("Cannot read its release: {why}"))?;
    let runs = &facts.board.release.version;
    match release {
        Some(release) if runs != release => Err(format!(
            "It runs {runs}, not {release}: it was flashed again since"
        )),
        _ => Ok(format!("Healthy after its soak, running {runs}")),
    }
}
