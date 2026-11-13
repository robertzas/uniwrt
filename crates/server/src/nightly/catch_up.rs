//! The radios still catching up after a channel switch (features/wifi.md, RF planner: Apply,
//! step 4; M6.1 b3b, D-113, D-116): moved by hostapd's `switch_chan`, a radio runs its new
//! channel while netifd and hostapd hold the old one until it restarts. Each look in the window
//! after tonight's optimizer has decided (or with the optimizer off), once no job runs (tonight's
//! apply finished), the managed online nodes with such radios get a restart job, one node at a
//! time. A node waits while one of those radios carries heavy traffic (the optimizer's 2 Mbit/s
//! rule) and is tried again at the next look; each node gets one restart job a night, whatever
//! its outcome, so a radio that doesn't catch up isn't restarted all night.

use std::collections::BTreeSet;

use tracing::{info, warn};
use uniwrt_core::NodeId;
use uniwrt_model::node::NodeMode;

use super::{Site, StartError, State};

pub(super) async fn catch_up(site: &impl Site, state: &mut State, night: i64) {
    if state.restarted.as_ref().is_none_or(|(n, _)| *n != night) {
        state.restarted = Some((night, BTreeSet::new()));
    }
    let Some((_, restarted)) = state.restarted.as_mut() else {
        return;
    };
    if site.job_running().await {
        return;
    }
    let modes = match site.nodes().await {
        Ok((modes, _)) => modes,
        Err(err) => return warn!(%err, "nightly RF: cannot read the nodes"),
    };
    let catching_up: Vec<(NodeId, Vec<String>)> = modes
        .into_iter()
        .filter(|(node, mode)| {
            *mode == NodeMode::Managed && !restarted.contains(node) && !site.offline(*node)
        })
        .map(|(node, _)| (node, site.catching_up(node)))
        .filter(|(_, radios)| !radios.is_empty())
        .collect();
    if catching_up.is_empty() {
        return;
    }
    let busy = match site.busy().await {
        Ok(busy) => busy,
        Err(err) => return warn!(%err, "nightly RF: cannot read the radios' traffic"),
    };
    let (quiet, loud): (Vec<_>, Vec<_>) = catching_up.into_iter().partition(|(node, radios)| {
        !radios
            .iter()
            .any(|radio| busy.contains(&(*node, radio.clone())))
    });
    if !loud.is_empty() {
        info!(
            ?loud,
            "nightly RF: radios to restart carry heavy traffic; trying again later"
        );
    }
    let nodes: Vec<NodeId> = quiet.into_iter().map(|(node, _)| node).collect();
    if nodes.is_empty() {
        return;
    }
    match site.restart(nodes.clone()).await {
        Ok(job) => {
            info!(?nodes, %job, "nightly RF: restarting the radios catching up");
            restarted.extend(nodes);
        }
        // Another job took a node meanwhile: the next look tries again.
        Err(StartError::Busy) => {}
        Err(StartError::Failed(err)) => {
            warn!(%err, "nightly RF: cannot restart the radios catching up");
            restarted.extend(nodes);
        }
    }
}
