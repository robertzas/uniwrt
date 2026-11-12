//! The manage and unmanage jobs (directive 8; D-069): management goes on only when the user asks,
//! node by node. Turning it on installs the transaction tool and the boot guard
//! ([`uniwrt_node::txn::Footprint`]) and records the node as managed; turning it off removes
//! them and records it in observe mode again, leaving its configuration as it is.

use std::sync::Arc;

use tracing::{info, warn};
use uniwrt_core::{NodeId, UserId};
use uniwrt_model::{
    job::{JobError, JobErrorCode, JobKind, NewJob, StepName},
    node::{Node, NodeMode},
};
use uniwrt_node::{
    Credential, HostKey, Target,
    txn::{self, Footprint},
};
use uniwrt_store::{HostKeyRepo as _, NodeRepo as _};

use crate::{
    Remote,
    errors::{connect_error, internal, txn_error},
    progress::Progress,
    runner::Shared,
};

const MANAGE: [StepName; 3] = [StepName::Connect, StepName::InstallTool, StepName::Record];
const UNMANAGE: [StepName; 3] = [StepName::Connect, StepName::RemoveTool, StepName::Record];

/// The key that keeps one job at a time on a node.
pub(crate) fn target(node: NodeId) -> String {
    format!("node/{node}")
}

pub(crate) fn job(node: &Node, enable: bool, by: Option<UserId>) -> NewJob {
    NewJob {
        kind: if enable {
            JobKind::Manage
        } else {
            JobKind::Unmanage
        },
        target: node.name.clone(),
        node: Some(node.id),
        revision: None,
        created_by: by,
        steps: if enable { MANAGE } else { UNMANAGE }.to_vec(),
        nodes: Vec::new(),
    }
}

pub(crate) async fn run(shared: Arc<Shared>, node: Node, enable: bool, mut progress: Progress) {
    let result = manage(&shared, &node, enable, &mut progress).await;
    match &result {
        Ok(()) => info!(node = %node.id, enable, "management switched"),
        Err(err) => warn!(node = %node.id, enable, code = ?err.code, "{}", err.message),
    }
    progress.finish(result.err()).await;
}

async fn manage(
    shared: &Shared,
    node: &Node,
    enable: bool,
    progress: &mut Progress,
) -> Result<(), JobError> {
    let remote = connect(shared, node, progress).await?;
    let switched = if enable {
        install(remote.as_ref(), progress).await
    } else {
        remove(remote.as_ref(), progress).await
    };
    remote.close().await;
    switched?;

    progress.start(StepName::Record).await;
    let mode = if enable {
        NodeMode::Managed
    } else {
        NodeMode::Observe
    };
    shared
        .store
        .set_node_mode(node.id, mode)
        .await
        .map_err(|err| internal("recording the node's mode", &err))?
        .ok_or_else(|| JobError {
            code: JobErrorCode::Internal,
            message: "The node was removed while the job ran".to_owned(),
        })?;
    progress
        .done(Some(
            if enable {
                "The node is managed"
            } else {
                "The node is in observe mode"
            }
            .to_owned(),
        ))
        .await;
    Ok(())
}

/// Logs in with the controller's key, requiring the pinned host key.
pub(crate) async fn connect(
    shared: &Shared,
    node: &Node,
    progress: &mut Progress,
) -> Result<Box<dyn Remote>, JobError> {
    progress.start(StepName::Connect).await;
    let remote = login(shared, node).await?;
    progress
        .done(Some(format!(
            "Logged in to {}",
            Target::new(node.host.clone(), node.port)
        )))
        .await;
    Ok(remote)
}

/// [`connect`] without a step: logs in again, as a job that lost its login does.
pub(crate) async fn login(shared: &Shared, node: &Node) -> Result<Box<dyn Remote>, JobError> {
    let Some(dial) = &shared.dial else {
        return Err(internal("no way to reach nodes", &"no dial"));
    };
    let pin = shared
        .store
        .host_key(node.id)
        .await
        .map_err(|err| internal("reading the host key", &err))?
        .ok_or_else(|| JobError {
            code: JobErrorCode::Internal,
            message: "No host key is pinned for the node: adopt it again".to_owned(),
        })?;
    let host_key = HostKey::from_pin(&pin).map_err(|err| internal("the pinned host key", &err))?;
    let target = Target::new(node.host.clone(), node.port);
    dial.connect(&target, &host_key, &Credential::Key(shared.key.clone()))
        .await
        .map_err(|err| connect_error(&err))
}

async fn status(remote: &dyn Remote) -> Result<Option<txn::Status>, JobError> {
    let command = txn::status();
    let output = remote
        .exec(command.request.clone())
        .await
        .map_err(|err| txn_error(&err, "Reading the transaction tool's status"))?;
    command
        .parse(output)
        .map_err(|err| txn_error(&err, "Reading the transaction tool's status"))
}

async fn install(remote: &dyn Remote, progress: &mut Progress) -> Result<(), JobError> {
    progress.start(StepName::InstallTool).await;
    let footprint = Footprint::get();
    let before = status(remote).await?;
    if let Some(open) = before.as_ref().and_then(|status| status.open.as_ref()) {
        return Err(open_transaction(open));
    }
    if before
        .as_ref()
        .is_some_and(|status| status.version == footprint.version())
    {
        progress
            .done(Some("Installed already, this version".to_owned()))
            .await;
        return Ok(());
    }
    let command = footprint.install();
    let what = "Installing the transaction tool";
    let output = remote
        .exec(command.request.clone())
        .await
        .map_err(|err| txn_error(&err, what))?;
    command.parse(output).map_err(|err| txn_error(&err, what))?;
    let after = status(remote).await?;
    if after.as_ref().map(|status| status.version.as_str()) != Some(footprint.version()) {
        return Err(JobError {
            code: JobErrorCode::NodeFailed,
            message: "The transaction tool does not report the version just installed".to_owned(),
        });
    }
    progress
        .done(Some(
            if before.is_some() {
                "Updated in /etc/uniwrt, with the boot guard"
            } else {
                "Installed in /etc/uniwrt, with the boot guard"
            }
            .to_owned(),
        ))
        .await;
    Ok(())
}

async fn remove(remote: &dyn Remote, progress: &mut Progress) -> Result<(), JobError> {
    progress.start(StepName::RemoveTool).await;
    let before = status(remote).await?;
    let Some(before) = before else {
        progress
            .skip(StepName::RemoveTool, "The node has no transaction tool")
            .await;
        return Ok(());
    };
    if let Some(open) = &before.open {
        return Err(open_transaction(open));
    }
    let command = txn::uninstall();
    let what = "Removing the transaction tool";
    let output = remote
        .exec(command.request.clone())
        .await
        .map_err(|err| txn_error(&err, what))?;
    command.parse(output).map_err(|err| txn_error(&err, what))?;
    progress
        .done(Some(
            "Removed /etc/uniwrt and the boot guard; the configuration stays".to_owned(),
        ))
        .await;
    Ok(())
}

fn open_transaction(open: &txn::Open) -> JobError {
    JobError {
        code: JobErrorCode::NodeFailed,
        message: format!(
            "Transaction {} is open on the node ({:?}, {} s left): try again once it ends",
            open.id, open.state, open.remaining_s
        ),
    }
}
