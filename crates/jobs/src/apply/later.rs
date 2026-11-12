//! The gateway's transactions after an apply's first (D-081; features/lifecycle.md,
//! Make-before-break): its ports bounce once the bridge is applied, so the hosts behind them ask
//! for a lease on the new subnet; then each later [`Round`] runs as a step of its own, planned
//! again from what the gateway holds then: dropping the old address once this controller comes
//! from the new subnet, and the WAN's change last; then the wired backhaul's round on every node
//! with a cable brought over ([`super::backhaul`]). A failure there rolls back that transaction
//! alone, and the apply ends partial.

use std::time::Duration;

use tokio::time::Instant;
use uniwrt_model::{
    apply::PlanAction,
    job::{JobError, JobErrorCode, JobNodeState, StepName},
    node::{Node, NodeRole},
};
use uniwrt_node::{
    ExecRequest,
    txn::{self, TxnId},
};

use super::{
    plan,
    round::Round,
    run::{Holds, Run, failed},
    verify::wait_applied,
};
use crate::{errors::chain, progress::Progress};

/// How long a bounced port stays down: long enough for every host to notice.
const BOUNCE_DOWN: Duration = Duration::from_secs(3);

/// A later round's failure: the rounds before stay.
fn later(round: Round, err: &JobError) -> JobError {
    let why = ["Rolled back. ", "Nothing changed. "]
        .iter()
        .find_map(|prefix| err.message.strip_prefix(prefix))
        .unwrap_or(&err.message);
    failed(
        JobErrorCode::PartialApply,
        format!(
            "{} failed and was undone; what was confirmed before it stays. Apply again once it \
             is fixed. {why}",
            round.failed_words()
        ),
    )
}

impl Run<'_> {
    /// Every transaction: installs, then the first round as the Stage, Apply, Verify and
    /// Confirm steps, then each later round of the gateway's as a step of its own.
    pub(crate) async fn rounds(&mut self, progress: &mut Progress) -> Result<(), JobError> {
        self.install(progress).await?;
        progress.start(StepName::Stage).await;
        self.update_tools(progress).await?;
        let summary = self.stage(progress).await?;
        progress.done(Some(summary)).await;
        progress.start(StepName::Apply).await;
        let summary = self.apply(progress).await?;
        progress.done(Some(summary)).await;
        progress.start(StepName::Verify).await;
        let summary = self.verify(progress).await?;
        progress.done(Some(summary)).await;
        progress.start(StepName::Confirm).await;
        let summary = self.confirm(progress).await?;
        progress.done(Some(summary)).await;

        let mut ran = Vec::new();
        while let Some(round) = self
            .lives
            .iter()
            .filter_map(|live| live.part.next.first().copied())
            .min()
        {
            ran.push(round);
            progress.start(round.step()).await;
            self.txn = TxnId::new(format!("{}-{}", self.base, round.suffix()))
                .map_err(|err| crate::errors::internal("naming the transaction", &err))?;
            for live in &mut self.lives {
                live.active = live.part.next.first() == Some(&round);
                live.moves = false;
                live.confirmed = false;
                live.deadline = None;
            }
            let group = self.active();
            let probed = if round == Round::Backhaul {
                Some(self.probe_cables(progress).await)
            } else {
                None
            };
            for &i in &group {
                let (id, name) = (self.lives[i].node.id, self.lives[i].node.name.clone());
                if let Err(why) = self.replan(i, round).await {
                    progress.node(id, JobNodeState::Failed, &why).await;
                    let err = failed(JobErrorCode::NodeFailed, format!("{name}: {why}"));
                    return Err(later(round, &err));
                }
                let live = &mut self.lives[i];
                if live.part.plan.action != PlanAction::Change {
                    // Nothing left after all: it holds the revision.
                    live.holds = Holds::Revision;
                    live.held = live.part.held.clone();
                    live.part.next.clear();
                    live.active = false;
                }
            }
            if self.active().is_empty() {
                let mut done = probed.unwrap_or_default();
                done.push("Nothing left to apply".to_owned());
                progress.done(Some(done.join(". "))).await;
                continue;
            }
            let result = async {
                self.stage(progress).await?;
                self.apply(progress).await?;
                self.verify(progress).await?;
                self.confirm(progress).await
            }
            .await;
            match result {
                Ok(summary) => {
                    let mut done = probed.unwrap_or_default();
                    done.push(summary);
                    progress.done(Some(done.join(". "))).await;
                }
                Err(err) => return Err(later(round, &err)),
            }
        }
        if !ran.contains(&Round::Settle) {
            progress
                .skip(
                    StepName::Settle,
                    "This controller's network keeps its subnet",
                )
                .await;
        }
        if !ran.contains(&Round::Wan) {
            progress
                .skip(StepName::Wan, "The WAN doesn't change among other things")
                .await;
        }
        if !ran.contains(&Round::Backhaul) {
            progress
                .skip(StepName::Backhaul, "No cable between nodes to bring over")
                .await;
        }
        Ok(())
    }

    /// Plans node `i`'s transaction of `round` from what it holds now, over its current login.
    async fn replan(&mut self, i: usize, round: Round) -> Result<(), String> {
        let live = &mut self.lives[i];
        let remote = live
            .remote
            .as_deref()
            .ok_or_else(|| "Its login dropped".to_owned())?;
        live.reading
            .refresh(self.shared, remote, live.node.role)
            .await?;
        let node = Node {
            host: live.host.clone(),
            ..live.node.clone()
        };
        let part = plan::plan(
            self.site,
            &self.secrets,
            &live.reading.report(&node, &live.backhaul),
            &self.shared.timing,
        );
        if part.plan.action == PlanAction::Refused {
            let why: Vec<&str> = part
                .plan
                .issues
                .iter()
                .map(|issue| issue.message.as_str())
                .collect();
            return Err(format!("Cannot plan the rest: {}", why.join("; ")));
        }
        if let Some(bridge) = &part.bridge {
            return Err(format!(
                "This controller still reaches it from outside {}",
                bridge.new.trunc()
            ));
        }
        if round == Round::Wan && part.next.contains(&Round::Wan) {
            // Only the WAN was left: something else changed on it meanwhile.
            return Err("Its configuration changed under the apply".to_owned());
        }
        live.part = part;
        live.part.next.retain(|next| *next > round);
        Ok(())
    }

    /// Has the hosts take their new addresses once the gateway has reloaded: an access point
    /// that moves renews its lease (over the login it still has) and answers only at the new
    /// one from here on, then the bridging gateway's ports bounce, so every host behind them
    /// asks again.
    pub(super) async fn move_hosts(&mut self, progress: &mut Progress) -> Result<(), JobError> {
        let active = self.active();
        let bridging = active
            .iter()
            .copied()
            .find(|&i| !self.lives[i].part.bounce.is_empty());
        // The gateway hands out the new addresses once its transaction has reloaded.
        let gateway = active.iter().copied().find(|&i| {
            self.lives[i].node.role == NodeRole::Gateway && self.lives[i].deadline.is_some()
        });
        let movers: Vec<usize> = active
            .iter()
            .copied()
            .filter(|&i| gateway.is_some() && self.lives[i].part.renew.is_some())
            .collect();
        if let Some(i) = gateway.filter(|_| bridging.is_some() || !movers.is_empty()) {
            let until = Instant::now() + self.shared.timing.verify_uplink;
            let candidates = self.lives[i].candidates();
            let reloaded = wait_applied(
                self.shared,
                &self.txn,
                &mut self.lives[i],
                &candidates,
                until,
            )
            .await;
            if let Err(why) = reloaded {
                return Err(self.bridge_failed(i, why, progress).await);
            }
        }
        for i in movers {
            let iface = self.lives[i].part.renew.clone().unwrap_or_default();
            let request = ExecRequest::new([
                "ubus",
                "call",
                &format!("network.interface.{iface}"),
                "renew",
            ]);
            // Best effort: its uplink's bounce or its lease's end moves it too.
            let _ = self.lives[i].exec(request).await;
            self.lives[i].moves = true;
        }
        let Some(i) = bridging else {
            return Ok(());
        };
        let live = &self.lives[i];
        let bounced = match txn::bounce(live.part.bounce.iter().map(String::as_str), BOUNCE_DOWN) {
            Ok(command) => live.tool(&command).await,
            Err(err) => Err(err),
        };
        if let Err(err) = bounced {
            let why = format!("Cannot bounce its ports: {}", chain(&err));
            return Err(self.bridge_failed(i, why, progress).await);
        }
        let live = &self.lives[i];
        let subnet = live
            .part
            .bridge
            .as_ref()
            .map(|bridge| bridge.new.trunc().to_string())
            .unwrap_or_default();
        let detail = format!(
            "Applied; {} bounce so the hosts ask for a lease on {subnet}. This controller's host \
             has {} s to take one (behind a switch, renew its lease by hand)",
            live.part.bounce.join(", "),
            self.shared.timing.verify_uplink.as_secs()
        );
        progress
            .node(live.node.id, JobNodeState::Applied, detail)
            .await;
        Ok(())
    }

    /// The gateway `i` failed before the hosts could move: everything rolls back.
    async fn bridge_failed(&mut self, i: usize, why: String, progress: &mut Progress) -> JobError {
        let (id, name) = (self.lives[i].node.id, self.lives[i].node.name.clone());
        progress.node(id, JobNodeState::Failed, &why).await;
        self.roll_back(progress).await;
        failed(
            JobErrorCode::RolledBack,
            format!("Rolled back. {name}: {why}"),
        )
    }
}
