//! Re-trusting a node's changed SSH host key (features/adoption.md, Identity, credentials,
//! trust; D-154). The adoption's first login and check run with the key the admin confirmed,
//! and the check finds the node's own device (its MAC and board) or fails with
//! `identity_mismatch` ([`super::takeover::into`]). A device that still trusts the
//! controller's key kept its configuration (a reflash keeping it, or new host keys alone): its
//! new key is pinned and it is watched again as it was, managed or not ([`keep`]). One that
//! no longer does was reset: the rest of the adoption adopts it back into its record, in
//! observe mode, as after a reset.

use uniwrt_model::job::{JobError, StepName};
use uniwrt_store::{HostKeyRepo as _, NodeRepo as _};

use super::{AdoptRequest, Reached};
use crate::{errors::internal, progress::Progress, runner::Shared};

/// Pins the device's new key and watches it again: nothing changes on the device.
pub(super) async fn keep(
    shared: &Shared,
    request: &AdoptRequest,
    reached: Reached,
    progress: &mut Progress,
) -> Result<(), JobError> {
    let Reached {
        remote, checked, ..
    } = reached;
    remote.close().await;
    let id = checked
        .into
        .ok_or_else(|| internal("finding the node", &"it has no record"))?;
    let trusted = "The device still trusts the controller's key";
    progress.skip(StepName::InstallKey, trusted).await;
    progress.skip(StepName::VerifyKey, trusted).await;
    let kept = "The device kept its configuration: nothing changes on it";
    for step in [StepName::SetPassword, StepName::Inventory, StepName::Backup] {
        progress.skip(step, kept).await;
    }

    progress.start(StepName::Record).await;
    shared
        .store
        .pin_host_key(id, request.host_key.pin(shared.clock.now()))
        .await
        .map_err(|err| internal("pinning the host key", &err))?;
    let node = shared
        .store
        .node(id)
        .await
        .map_err(|err| internal("reading the node", &err))?
        .ok_or_else(|| internal("finding the node", &"it is gone"))?;
    let mode = match node.mode {
        uniwrt_model::node::NodeMode::Managed => "managed",
        uniwrt_model::node::NodeMode::Observe => "in observe mode",
    };
    progress
        .done(Some(format!(
            "Pinned its new host key, {}; {} stays {mode}",
            request.host_key.fingerprint(),
            node.name
        )))
        .await;

    progress.start(StepName::Watch).await;
    shared.watch.moved(node, request.host_key.clone()).await;
    progress.done(None).await;
    Ok(())
}
