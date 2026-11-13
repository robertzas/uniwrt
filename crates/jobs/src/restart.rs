//! Restarting the radios still catching up after a channel switch (features/wifi.md, RF
//! planner: Apply, step 4; M6.1 b3b, D-113, D-116). A radio moved by hostapd's `switch_chan`
//! runs its new channel while netifd and hostapd hold the old one until it restarts (the
//! agent's `radios` set marks it `restart_pending`). The nightly window starts this job for the
//! managed nodes whose catching-up radios are quiet. One node at a time, so its neighbors carry
//! the clients: log in → read its radios → `wifi reload`, which restarts only the radios whose
//! configuration netifd holds stale → wait until each runs its saved channel and is up again.
//! The configuration doesn't change, so there is nothing to roll back; the first node whose
//! radios don't come back ends the job.

#[cfg(test)]
mod tests;

use std::sync::Arc;

use tokio::time::Instant;
use tracing::{info, warn};
use uniwrt_model::{
    facts::RadioFacts,
    job::{JobError, JobErrorCode, JobKind, JobNode, JobNodeState, NewJob, StepName},
    node::Node,
};
use uniwrt_node::{ExecRequest, proto::ops::Set};

use crate::{
    Remote, apply::survey, errors::chain, manage::login, progress::Progress, runner::Shared,
};

pub(crate) fn job(nodes: &[Node]) -> NewJob {
    let names: Vec<&str> = nodes.iter().map(|node| node.name.as_str()).collect();
    NewJob {
        kind: JobKind::RadioRestart,
        target: format!("radios on {}", names.join(", ")),
        node: (nodes.len() == 1).then(|| nodes[0].id),
        revision: None,
        created_by: None,
        steps: vec![StepName::Restart],
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
    let result = restart(&shared, &nodes, &mut progress).await;
    match &result {
        Ok(()) => info!(nodes = nodes.len(), "radios restarted"),
        Err(err) => warn!(code = ?err.code, "{}", err.message),
    }
    progress.finish(result.err()).await;
}

async fn restart(shared: &Shared, nodes: &[Node], progress: &mut Progress) -> Result<(), JobError> {
    progress.start(StepName::Restart).await;
    let (mut restarted, mut current) = (0, 0);
    for node in nodes {
        let outcome = match login(shared, node).await {
            Ok(remote) => {
                let outcome = restart_on(shared, node, remote.as_ref(), progress).await;
                remote.close().await;
                outcome
            }
            Err(err) => Err(err.message),
        };
        match outcome {
            Ok(true) => restarted += 1,
            Ok(false) => current += 1,
            Err(why) => {
                progress.node(node.id, JobNodeState::Failed, &why).await;
                return Err(JobError {
                    code: JobErrorCode::NodeFailed,
                    message: format!("{}: {why}", node.name),
                });
            }
        }
    }
    let mut done = Vec::new();
    if restarted > 0 {
        done.push(format!(
            "Restarted the radios of {}, one at a time",
            count(restarted)
        ));
    }
    if current > 0 {
        done.push(format!("{} caught up already", count(current)));
    }
    progress.done(Some(done.join("; "))).await;
    Ok(())
}

/// Restarts `node`'s radios that catch up; `Ok(false)` when none does any more.
async fn restart_on(
    shared: &Shared,
    node: &Node,
    remote: &dyn Remote,
    progress: &mut Progress,
) -> Result<bool, String> {
    let facts = survey::collect(shared, remote, &[Set::Radios], &[Set::Radios]).await?;
    let pending: Vec<String> = facts
        .radios
        .iter()
        .filter(|radio| radio.restart_pending)
        .map(|radio| radio.name.clone())
        .collect();
    if pending.is_empty() {
        progress
            .node(
                node.id,
                JobNodeState::Unchanged,
                "Its radios run their saved channels already",
            )
            .await;
        return Ok(false);
    }
    progress
        .node(
            node.id,
            JobNodeState::Applied,
            format!("Restarting {}", pending.join(", ")),
        )
        .await;
    // netifd starts tearing a radio down within the reload's call, so a radio up on its saved
    // channel afterwards has restarted.
    remote
        .exec(ExecRequest::new(["wifi", "reload"]))
        .await
        .and_then(|out| out.check("wifi").map(drop))
        .map_err(|err| format!("The reload failed: {}", chain(&err)))?;
    let wait = shared.timing.verify_uplink;
    let until = Instant::now() + wait;
    let mut late = pending.clone();
    loop {
        if let Ok(facts) = survey::collect(shared, remote, &[Set::Radios], &[Set::Radios]).await {
            late.retain(|name| !back(&facts.radios, name));
            if late.is_empty() {
                let channels: Vec<String> = pending
                    .iter()
                    .map(|name| match facts.radios.iter().find(|r| &r.name == name) {
                        Some(RadioFacts {
                            channel: Some(channel),
                            ..
                        }) => format!("{name} on channel {channel}"),
                        _ => name.clone(),
                    })
                    .collect();
                progress
                    .node(
                        node.id,
                        JobNodeState::Verified,
                        format!("Restarted {}", channels.join(", ")),
                    )
                    .await;
                return Ok(true);
            }
        }
        if Instant::now() >= until {
            return Err(format!(
                "{} didn't come back on the saved channel within {} s",
                late.join(", "),
                wait.as_secs()
            ));
        }
        tokio::time::sleep(shared.timing.poll).await;
    }
}

/// Whether radio `name` is up and runs its saved channel.
fn back(radios: &[RadioFacts], name: &str) -> bool {
    radios
        .iter()
        .any(|radio| radio.name == name && radio.up && !radio.restart_pending)
}

fn count(n: usize) -> String {
    if n == 1 {
        "1 node".to_owned()
    } else {
        format!("{n} nodes")
    }
}
