//! Restarting nodes (features/network.md, Other services; M8.1b2a, D-137): one node at a time,
//! as an apply orders them (farthest from the gateway first, the gateway last), each back
//! before the next goes down, so its neighbors carry the clients. For each: log in → `reboot`
//! (a lost login is what success looks like) → down within [`RebootTiming::down`] → a login
//! with its pinned host key within [`RebootTiming::back`] → if it was never seen down, an
//! uptime shorter than the time since the command. Nothing changes on the node, so nothing
//! rolls back; the first node that fails ends the job and leaves the others as they are.

#[cfg(test)]
mod tests;

use std::{fmt::Write as _, sync::Arc, time::Duration};

use tokio::time::Instant;
use tracing::{info, warn};
use uniwrt_core::UserId;
use uniwrt_model::{
    job::{JobError, JobErrorCode, JobKind, JobNode, JobNodeState, NewJob, StepName},
    node::Node,
};
use uniwrt_node::{ExecRequest, Target, proto::ops::Set};

use crate::{
    Remote, apply::survey, errors::last_line, manage::login, progress::Progress, runner::Shared,
};

/// How long `reboot` may take to answer before the login is given up on: it signals init and
/// returns at once, unless the node is already going down.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

/// How patiently a restart waits on each node.
#[derive(Debug, Clone)]
pub struct RebootTiming {
    /// Between two tries while the node goes down and comes back.
    pub poll: Duration,
    /// How long `reboot` may take to take the node down.
    pub down: Duration,
    /// How long the node may take to accept a login again, from the command.
    pub back: Duration,
}

impl Default for RebootTiming {
    fn default() -> Self {
        Self {
            poll: Duration::from_secs(3),
            down: Duration::from_secs(60),
            back: Duration::from_secs(600),
        }
    }
}

pub(crate) fn job(nodes: &[Node], by: Option<UserId>) -> NewJob {
    let names: Vec<&str> = nodes.iter().map(|node| node.name.as_str()).collect();
    NewJob {
        kind: JobKind::Reboot,
        target: names.join(", "),
        node: (nodes.len() == 1).then(|| nodes[0].id),
        revision: None,
        created_by: by,
        steps: vec![StepName::Reboot],
        nodes: nodes
            .iter()
            .map(|node| JobNode {
                node: node.id,
                name: node.name.clone(),
                state: JobNodeState::Pending,
                detail: None,
            })
            .collect(),
    }
}

pub(crate) async fn run(shared: Arc<Shared>, nodes: Vec<Node>, mut progress: Progress) {
    let result = reboot_all(&shared, &nodes, &mut progress).await;
    match &result {
        Ok(()) => info!(nodes = nodes.len(), "nodes restarted"),
        Err(err) => warn!(code = ?err.code, "{}", err.message),
    }
    progress.finish(result.err()).await;
}

async fn reboot_all(
    shared: &Shared,
    nodes: &[Node],
    progress: &mut Progress,
) -> Result<(), JobError> {
    progress.start(StepName::Reboot).await;
    for (i, node) in nodes.iter().enumerate() {
        progress
            .node(node.id, JobNodeState::Applied, "Restarting")
            .await;
        match reboot(shared, node).await {
            Ok(back) => {
                let detail = format!("Back after {} s", back.as_secs().max(1));
                progress.node(node.id, JobNodeState::Verified, detail).await;
            }
            Err(mut err) => {
                progress
                    .node(node.id, JobNodeState::Failed, &err.message)
                    .await;
                let left = nodes.len() - i - 1;
                err.message = format!("{}: {}", node.name, err.message);
                if left > 0 {
                    let verb = if left == 1 { "was" } else { "were" };
                    let _ = write!(
                        err.message,
                        " The {} after it {verb} not restarted.",
                        count(left)
                    );
                }
                return Err(err);
            }
        }
    }
    let done = if nodes.len() == 1 {
        format!("{} restarted", nodes[0].name)
    } else {
        format!("Restarted {}, one at a time", count(nodes.len()))
    };
    progress.done(Some(done)).await;
    Ok(())
}

/// Restarts `node` and waits for it; how long it took to accept a login again.
async fn reboot(shared: &Shared, node: &Node) -> Result<Duration, JobError> {
    let timing = &shared.timing.reboot;
    let remote = login(shared, node).await?;
    let start = Instant::now();
    let sent = remote
        .exec(ExecRequest::new(["reboot"]).timeout(COMMAND_TIMEOUT))
        .await;
    drop(remote);
    if let Ok(output) = sent
        && !output.status.success()
    {
        let said = [output.stderr, output.stdout].concat();
        return Err(failed(format!("reboot refused: {}", last_line(&said))));
    }
    let target = Target::new(node.host.clone(), node.port);
    let mut went_down = false;
    while start.elapsed() < timing.down {
        if !answers(shared, &target).await {
            went_down = true;
            break;
        }
        tokio::time::sleep(timing.poll).await;
    }
    loop {
        match login(shared, node).await {
            Ok(remote) => {
                let back = start.elapsed();
                let checked = if went_down {
                    Ok(())
                } else {
                    restarted(shared, remote.as_ref(), back).await
                };
                remote.close().await;
                return checked.map(|()| back);
            }
            Err(err) if err.code == JobErrorCode::HostKeyMismatch => {
                return Err(JobError {
                    code: JobErrorCode::HostKeyMismatch,
                    message: "It came back with another host key. Make sure it is the device, \
                              then read its key again."
                        .to_owned(),
                });
            }
            Err(_) if start.elapsed() >= timing.back => {
                return Err(JobError {
                    code: JobErrorCode::NotBack,
                    message: format!(
                        "It didn't come back within {} min of the restart. Check its power and \
                         cables; if it doesn't boot, its failsafe mode recovers it (its OpenWrt \
                         wiki page tells how).",
                        timing.back.as_secs().div_ceil(60)
                    ),
                });
            }
            Err(_) => tokio::time::sleep(timing.poll).await,
        }
    }
}

/// Fails unless the node booted within the last `since`: it was never seen down.
async fn restarted(shared: &Shared, remote: &dyn Remote, since: Duration) -> Result<(), JobError> {
    let facts = survey::collect(shared, remote, &[Set::System], &[Set::System])
        .await
        .map_err(failed)?;
    let up = Duration::from_secs(facts.system.uptime_s);
    if up >= since {
        return Err(failed(format!(
            "It didn't restart: up for {} min",
            up.as_secs() / 60
        )));
    }
    Ok(())
}

/// Whether the node's SSH server answers.
pub(crate) async fn answers(shared: &Shared, target: &Target) -> bool {
    let Some(dial) = &shared.dial else {
        return false;
    };
    matches!(dial.banner(target).await, Ok(Some(_)))
}

fn failed(message: impl Into<String>) -> JobError {
    JobError {
        code: JobErrorCode::NodeFailed,
        message: message.into(),
    }
}

fn count(n: usize) -> String {
    if n == 1 {
        "1 node".to_owned()
    } else {
        format!("{n} nodes")
    }
}
