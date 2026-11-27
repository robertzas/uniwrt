//! Replace and factory reset (features/lifecycle.md, Replace / RMA / decommission; D-131).
//!
//! - **Replace:** the adoption's steps with the new device, recorded into the node's record
//!   ([`crate::adopt::Replace`]): it keeps its id, name, role and history, and a revision moves
//!   its settings to the new device. The gateway may be replaced.
//! - **Reset:** connect → remove the transaction tool → wipe with `firstboot` (as a
//!   decommission does) → record: a pre-approval of the node's MAC and board that never
//!   expires ([`ZtpApproval::awaiting`]), so whichever path finds the device again adopts it
//!   back into the record; the node is watched again meanwhile (it shows offline).
//! - **Re-trust** (D-154): the adoption's steps with the node's own device at its address and
//!   the new host key the admin confirmed ([`crate::adopt::Replace::retrust`]).

use std::sync::Arc;

use tracing::{info, warn};
use uniwrt_core::NodeId;
use uniwrt_model::{
    job::{Job, JobError, JobKind, NewJob, StepName},
    node::Node,
    ztp::ZtpApproval,
};
use uniwrt_node::HostKey;
use uniwrt_store::ZtpRepo as _;
use zeroize::Zeroizing;

use super::{Author, DECOMMISSION, keys, watch_again, wipe};
use crate::{
    AdoptRequest, Error, Jobs, Replace, Result, adopt, errors::internal, progress::Progress,
    runner::Shared,
};

impl Jobs {
    /// Starts adopting the device `request` names into `node`'s record, in place of its failed
    /// device (D-131), and returns the job as it starts.
    pub async fn replace(
        &self,
        node: NodeId,
        mut request: AdoptRequest,
        by: Option<Author>,
    ) -> Result<Job> {
        if self.shared.dial.is_none() {
            return Err(Error::Unavailable);
        }
        let node = self.held(node, false).await?;
        request.port = None;
        request.replace = Some(Replace {
            node: node.id,
            by: by.clone(),
            retrust: false,
        });
        let mut held = keys(&node);
        held.push(request.target.to_string().to_lowercase());
        let job = NewJob {
            kind: JobKind::Replace,
            target: node.name.clone(),
            node: Some(node.id),
            revision: None,
            created_by: by.map(|author| author.id),
            steps: adopt::STEPS.to_vec(),
            nodes: Vec::new(),
        };
        self.spawn(held, job, move |shared, progress| {
            adopt::run(shared, request, progress)
        })
        .await
    }

    /// Starts re-trusting `node`'s device, whose host key changed (D-154), and returns the job
    /// as it starts. The key pinned already may be trusted again: the link, quarantined when
    /// another key answered, reconnects.
    pub async fn retrust(&self, node: NodeId, retrust: Retrust, by: Option<Author>) -> Result<Job> {
        if self.shared.dial.is_none() {
            return Err(Error::Unavailable);
        }
        let node = self.held(node, false).await?;
        let request = AdoptRequest {
            target: uniwrt_node::Target::new(node.host.clone(), node.port),
            host_key: retrust.host_key,
            password: retrust.password,
            remember_password: retrust.remember_password,
            allow_unsupported_release: retrust.allow_unsupported_release,
            name: None,
            role: None,
            ztp: None,
            port: None,
            replace: Some(Replace {
                node: node.id,
                by: by.clone(),
                retrust: true,
            }),
        };
        let mut held = keys(&node);
        held.push(request.target.to_string().to_lowercase());
        let job = NewJob {
            kind: JobKind::Retrust,
            target: node.name.clone(),
            node: Some(node.id),
            revision: None,
            created_by: by.map(|author| author.id),
            steps: adopt::STEPS.to_vec(),
            nodes: Vec::new(),
        };
        self.spawn(held, job, move |shared, progress| {
            adopt::run(shared, request, progress)
        })
        .await
    }

    /// Starts resetting `node` to its image's defaults (D-131), its record then awaiting the
    /// device's re-adoption, and returns the job as it starts.
    pub async fn reset(&self, node: NodeId, by: Option<Author>) -> Result<Job> {
        if self.shared.dial.is_none() {
            return Err(Error::Unavailable);
        }
        let node = self.held(node, true).await?;
        let job = NewJob {
            kind: JobKind::Reset,
            target: node.name.clone(),
            node: Some(node.id),
            revision: None,
            created_by: by.as_ref().map(|author| author.id),
            steps: DECOMMISSION.to_vec(),
            nodes: Vec::new(),
        };
        self.spawn(keys(&node), job, move |shared, progress| {
            run(shared, node, by, progress)
        })
        .await
    }
}

/// What re-trusting a node's device takes from the admin (D-154): its address is the node's.
pub struct Retrust {
    /// The new host key the admin confirmed; the device must present it.
    pub host_key: HostKey,
    /// The root password, for a device that no longer trusts the controller's key; `None` tries
    /// the key, then no password (factory defaults).
    pub password: Option<Zeroizing<String>>,
    /// Keep the password in the vault, for emergencies.
    pub remember_password: bool,
    /// Take the device even if its release is one adoption refuses (D-145).
    pub allow_unsupported_release: bool,
}

async fn run(shared: Arc<Shared>, node: Node, by: Option<Author>, mut progress: Progress) {
    // A failed wipe watches the node again itself.
    let result = match wipe(&shared, &node, true, &mut progress).await {
        Ok(()) => {
            let recorded = record(&shared, &node, by.as_ref(), &mut progress).await;
            watch_again(&shared, &node).await;
            recorded
        }
        Err(err) => Err(err),
    };
    match &result {
        Ok(()) => info!(node = %node.id, "reset; the record awaits the device"),
        Err(err) => warn!(node = %node.id, code = ?err.code, "{}", err.message),
    }
    progress.finish(result.err()).await;
}

async fn record(
    shared: &Shared,
    node: &Node,
    by: Option<&Author>,
    progress: &mut Progress,
) -> Result<(), JobError> {
    progress.start(StepName::Record).await;
    let approval = ZtpApproval::awaiting(
        node,
        shared.clock.now(),
        by.map(|author| author.name.clone()),
    );
    shared
        .store
        .put_ztp_approval(approval)
        .await
        .map_err(|err| internal("pre-approving the device", &err))?;
    progress
        .done(Some(format!(
            "{} awaits the device: adopted again by any path (its MAC address {} and board {} \
             are pre-approved), it goes back into this record",
            node.name, node.mac, node.board_name
        )))
        .await;
    Ok(())
}
