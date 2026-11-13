//! The apply job (features/lifecycle.md, Render → diff → apply; D-012, D-069): applies the
//! site's latest revision to every managed node as one fleet-wide two-phase commit.
//!
//! Steps: connect (every managed node, or nothing changes) → plan ([`survey`]: read what each
//! holds, render, diff; any refused node stops it) → install (the feed packages a node's
//! configuration needs, [`install`]) → stage (stage and check every transaction)
//! → apply (leaves → gateway, each with its dead-man timer) → verify (all at once, each over a
//! new login at the address it moves to, against its health before) → confirm (all) → settle
//! and wan (the gateway's later transactions, D-081: dropping the old address of this
//! controller's network once it reaches the new one, and the WAN's change last) → backhaul
//! (the cables brought over as overlays probed from both ends, then each carries the mesh alone
//! or goes back to a trunk, D-097) → record
//! (the revision each node holds, and the addresses that moved). A failure before a confirm
//! rolls every applied node back, and one it can't reach rolls back by itself at its deadline;
//! a failure in a later transaction rolls back that one alone.

mod address;
mod adopted;
mod backhaul;
mod cables;
mod controller;
pub(crate) mod health;
mod install;
mod later;
mod order;
pub(crate) mod plan;
mod round;
mod run;
pub(crate) mod survey;
#[cfg(test)]
pub(crate) mod tests;
mod verify;

use std::{collections::BTreeMap, sync::Arc, time::Duration};

use tracing::{info, warn};
use uniwrt_core::UserId;
use uniwrt_model::{
    apply::{Applied, AppliedNode, ApplyPlan, PlanAction},
    job::{JobError, JobErrorCode, JobKind, JobNode, JobNodeState, NewJob, StepName},
    node::{Node, NodeMode},
    site::revision::Revision,
};
use uniwrt_node::txn::TxnId;
use uniwrt_store::ApplyRepo as _;

pub(crate) use self::adopted::{Configured, adopted, held};
use self::{
    run::{Holds, Live, Run, nodes},
    survey::Survey,
};
use crate::{errors::internal, progress::Progress, runner::Shared};

/// The key that keeps one apply at a time.
pub(crate) const TARGET: &str = "apply";

const STEPS: [StepName; 11] = [
    StepName::Connect,
    StepName::Plan,
    StepName::Install,
    StepName::Stage,
    StepName::Apply,
    StepName::Verify,
    StepName::Confirm,
    StepName::Settle,
    StepName::Wan,
    StepName::Backhaul,
    StepName::Record,
];

/// How patiently an apply waits on nodes. [`Default`] is production's; the system harness's
/// end-to-end tests shorten it ([`Jobs::start_with`](crate::Jobs::start_with)).
#[derive(Debug, Clone)]
pub struct ApplyTiming {
    /// Between two tries while a node reconnects, reloads or recovers.
    pub poll: Duration,
    /// How long a node may take to be reachable and healthy again.
    pub verify: Duration,
    /// The same, after a change of its WAN or the mesh.
    pub verify_uplink: Duration,
    /// The dead-man timer of a transaction (features/lifecycle.md), within the tool's bounds
    /// (`uniwrt_node::txn::MIN_TIMEOUT`, `MAX_TIMEOUT`); longer than `verify`.
    pub timer: Duration,
    /// The same for one that changes the WAN or the mesh, which take longer to settle.
    pub timer_uplink: Duration,
    /// How long a WAN test waits for an address (a third of it for the link, three quarters
    /// for IPv6).
    pub wan_wait: Duration,
}

impl Default for ApplyTiming {
    fn default() -> Self {
        Self {
            poll: Duration::from_secs(2),
            verify: Duration::from_secs(60),
            verify_uplink: Duration::from_secs(150),
            timer: Duration::from_secs(120),
            timer_uplink: Duration::from_secs(300),
            wan_wait: Duration::from_secs(60),
        }
    }
}

pub(crate) fn job(revision: &Revision, nodes: &[Node], by: Option<UserId>) -> NewJob {
    NewJob {
        kind: JobKind::Apply,
        target: format!("revision {}", revision.meta.id),
        node: None,
        revision: Some(revision.meta.id),
        created_by: by,
        steps: STEPS.to_vec(),
        nodes: nodes
            .iter()
            .map(|node| {
                let managed = node.mode == NodeMode::Managed;
                JobNode {
                    node: node.id,
                    name: node.name.clone(),
                    state: if managed {
                        JobNodeState::Pending
                    } else {
                        JobNodeState::Skipped
                    },
                    detail: (!managed).then(|| "Observe mode".to_owned()),
                }
            })
            .collect(),
    }
}

/// The plan of applying `revision` now; nothing changes on the nodes.
pub(crate) async fn plan(
    shared: &Shared,
    revision: &Revision,
    nodes: Vec<Node>,
) -> Result<ApplyPlan, JobError> {
    let (managed, observing) = survey::managed(nodes);
    let (logins, failed) = survey::connect_all(shared, managed).await;
    let Survey { plan, members, .. } =
        survey::plan_all(shared, revision, logins, failed, observing).await?;
    for member in members {
        member.login.remote.close().await;
    }
    Ok(plan)
}

pub(crate) async fn run(
    shared: Arc<Shared>,
    revision: Revision,
    nodes: Vec<Node>,
    mut progress: Progress,
) {
    let result = apply(&shared, &revision, nodes, &mut progress).await;
    match &result {
        Ok(()) => info!(revision = %revision.meta.id, "applied"),
        Err(err) => warn!(revision = %revision.meta.id, code = ?err.code, "{}", err.message),
    }
    progress.finish(result.err()).await;
}

async fn apply(
    shared: &Shared,
    revision: &Revision,
    nodes: Vec<Node>,
    progress: &mut Progress,
) -> Result<(), JobError> {
    let (managed, observing) = survey::managed(nodes);
    progress.start(StepName::Connect).await;
    let (logins, failed) = survey::connect_all(shared, managed).await;
    if !failed.is_empty() {
        for login in logins {
            login.remote.close().await;
        }
        let mut reasons = Vec::new();
        for (node, why) in failed {
            progress.node(node.id, JobNodeState::Failed, &why).await;
            reasons.push(format!("{}: {why}", node.name));
        }
        return Err(JobError {
            code: JobErrorCode::Unreachable,
            message: format!("Nothing changed. {}", reasons.join(". ")),
        });
    }
    progress
        .done(Some(format!("Logged in to {}", run::nodes(logins.len()))))
        .await;

    progress.start(StepName::Plan).await;
    let Survey {
        plan,
        members,
        secrets,
        probes,
    } = survey::plan_all(shared, revision, logins, Vec::new(), observing).await?;
    if !plan.ready() {
        for member in members {
            member.login.remote.close().await;
        }
        for node in plan
            .nodes
            .iter()
            .filter(|n| n.action == PlanAction::Refused)
        {
            let why = node
                .reason
                .clone()
                .or_else(|| node.issues.first().map(|issue| issue.message.clone()))
                .unwrap_or_default();
            progress.node(node.node, JobNodeState::Failed, why).await;
        }
        return Err(survey::refused(&plan));
    }
    let mut unchanged = Vec::new();
    let mut lives = Vec::new();
    for member in members {
        if member.part.plan.action == PlanAction::Change
            || member.part.follows()
            || member.part.next.contains(&round::Round::Backhaul)
        {
            lives.push(Live::from(member));
        } else {
            let detail = format!("Holds revision {} already", revision.meta.id);
            progress
                .node(member.login.node.id, JobNodeState::Unchanged, detail)
                .await;
            member.login.remote.close().await;
            unchanged.push((member.login.node, member.part.held));
        }
    }
    progress.done(Some(summary(&plan))).await;

    if lives.is_empty() {
        for step in [
            StepName::Install,
            StepName::Stage,
            StepName::Apply,
            StepName::Verify,
            StepName::Confirm,
            StepName::Settle,
            StepName::Wan,
            StepName::Backhaul,
        ] {
            progress.skip(step, "Nothing to change").await;
        }
        return record(shared, revision, progress, &[], &unchanged).await;
    }
    let txn = TxnId::new(format!("uniwrt-{}", progress.job_id().get()))
        .map_err(|err| internal("naming the transaction", &err))?;
    let mut run = Run::new(shared, &revision.config, secrets, txn, lives, probes);
    let result = run.rounds(progress).await;
    run.close().await;
    match result {
        Ok(()) => record(shared, revision, progress, &run.lives, &unchanged).await,
        Err(err) if err.code == JobErrorCode::PartialApply => {
            // Record what was confirmed before failing: the nodes that hold the revision,
            // and the packages of one that holds part of it.
            let confirmed: Vec<_> = run
                .lives
                .into_iter()
                .filter(|live| live.holds != Holds::Before)
                .collect();
            if let Err(why) = store(shared, revision, progress, &confirmed, &unchanged).await {
                warn!("{}", why.message);
            }
            Err(err)
        }
        Err(err) => Err(err),
    }
}

/// The Record step: the revision each node holds now, and where the moved ones are.
async fn record(
    shared: &Shared,
    revision: &Revision,
    progress: &mut Progress,
    confirmed: &[Live],
    unchanged: &[(Node, BTreeMap<String, String>)],
) -> Result<(), JobError> {
    progress.start(StepName::Record).await;
    store(shared, revision, progress, confirmed, unchanged).await?;
    let detail = format!(
        "Revision {} on {}",
        revision.meta.id,
        nodes(confirmed.len() + unchanged.len())
    );
    progress.done(Some(detail)).await;
    Ok(())
}

async fn store(
    shared: &Shared,
    revision: &Revision,
    progress: &Progress,
    confirmed: &[Live],
    unchanged: &[(Node, BTreeMap<String, String>)],
) -> Result<(), JobError> {
    let moved = |live: &Live| (live.host != live.node.host).then(|| live.host.clone());
    let applied = Applied {
        revision: revision.meta.id,
        job: progress.job_id(),
        nodes: confirmed
            .iter()
            .map(|live| AppliedNode {
                node: live.node.id,
                holds: live.holds == Holds::Revision,
                host: moved(live),
                packages: live.held.clone(),
            })
            .chain(unchanged.iter().map(|(node, held)| AppliedNode {
                node: node.id,
                holds: true,
                host: None,
                packages: held.clone(),
            }))
            .collect(),
    };
    shared
        .store
        .record_applied(applied)
        .await
        .map_err(|err| internal("recording the applied revision", &err))?;
    for live in confirmed.iter().filter(|live| live.host != live.node.host) {
        let node = Node {
            host: live.host.clone(),
            ..live.node.clone()
        };
        info!(node = %node.id, host = %node.host, "the apply moved the node");
        shared.watch.moved(node, live.host_key.clone()).await;
    }
    Ok(())
}

fn summary(plan: &ApplyPlan) -> String {
    let count = |action| plan.nodes.iter().filter(|n| n.action == action).count();
    let mut parts = vec![format!("{} change", nodes(count(PlanAction::Change)))];
    let unchanged = count(PlanAction::Unchanged);
    if unchanged > 0 {
        parts.push(format!("{} hold the revision already", nodes(unchanged)));
    }
    let skipped = count(PlanAction::Skipped);
    if skipped > 0 {
        parts.push(format!("{} in observe mode", nodes(skipped)));
    }
    parts.join(", ")
}
