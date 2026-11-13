//! The mesh's synchronized cutover (features/wifi.md, RF planner: Apply; features/mesh.md,
//! Mesh settings; M6.1 b2, D-114): where an apply drops mesh links (a mesh point moves, leaves
//! or goes off on some node), every node of the round reloads at one instant rather than
//! leaves → gateway, so no node sits on a mesh its neighbors have left while the others apply.
//!
//! The controller sends every node its `apply` at once, each with the countdown left to that
//! instant ([`txn::apply_at`]): the tool snapshots, arms and installs at once, and reloads only
//! when its countdown ends, on its uptime clock (no NTP needed: the error is the command's
//! latency). Its dead-man timer counts from then. Nothing changes before the instant, so a node
//! that refuses its apply rolls the others back before any of them moved. Verify starts at the
//! instant, with the uplink window; a mesh that doesn't re-form fails it (each node must hear as
//! many batman-adv neighbors as before), and every node rolls back.

use std::time::Duration;

use futures_util::future::join_all;
use tokio::time::{Instant, sleep_until};
use uniwrt_model::job::{JobError, JobErrorCode, JobNodeState};
use uniwrt_node::txn;

use super::run::{Run, failed, nodes};
use crate::{errors::chain, progress::Progress};

impl Run<'_> {
    /// Whether the round's nodes cut over together: some node's mesh links drop, and more than
    /// one node applies.
    pub(super) fn cutover(&self) -> bool {
        let applying = self.lives.iter().filter(|l| l.active && !l.follows());
        applying.clone().count() > 1 && applying.clone().any(|l| l.part.mesh.cuts())
    }

    /// Applies every transaction of the round with one countdown, then waits for its end.
    pub(super) async fn apply_at(&mut self, progress: &mut Progress) -> Result<String, JobError> {
        let lead = self.shared.timing.cutover_lead;
        let at = Instant::now() + lead;
        let txn = &self.txn;
        let results = join_all(
            self.lives
                .iter()
                .filter(|live| live.active && !live.follows())
                .map(|live| async move {
                    let wait = at.saturating_duration_since(Instant::now());
                    live.tool(&txn::apply_at(txn, live.part.timeout, wait))
                        .await
                }),
        )
        .await;
        let applying: Vec<usize> = self
            .active()
            .into_iter()
            .filter(|&i| !self.lives[i].follows())
            .collect();
        let mut refused = None;
        for (&i, result) in applying.iter().zip(results) {
            let live = &mut self.lives[i];
            let id = live.node.id;
            let timeout = live.part.timeout;
            match result {
                Ok(armed) => {
                    live.deadline = Some(at + timeout);
                    let detail = match armed.wait_ms {
                        Some(0) => format!(
                            "Applied late: it reloaded at once, without waiting for the others; \
                             it rolls back by itself {} s later unless confirmed",
                            timeout.as_secs()
                        ),
                        _ => format!(
                            "Applied; it reloads with the others in {} s, then rolls back by \
                             itself {} s later unless confirmed",
                            lead.as_secs(),
                            timeout.as_secs()
                        ),
                    };
                    progress.node(id, JobNodeState::Applied, detail).await;
                }
                Err(uniwrt_node::Error::Txn { msg, .. }) => {
                    let why = format!("It could not apply: {msg}");
                    progress.node(id, JobNodeState::Failed, &why).await;
                    refused.get_or_insert(format!("{}: {why}", live.node.name));
                }
                Err(err) => {
                    // The answer is lost, not necessarily the apply: verifying tells.
                    live.deadline = Some(at + timeout);
                    live.close().await;
                    let detail = format!("The connection dropped as it applied: {}", chain(&err));
                    progress.node(id, JobNodeState::Applied, detail).await;
                }
            }
        }
        for i in self.active() {
            if self.lives[i].follows() {
                let id = self.lives[i].node.id;
                let detail = "Follows once the gateway applied";
                progress.node(id, JobNodeState::Applied, detail).await;
            }
        }
        if let Some(first) = refused {
            // Before the instant nobody has reloaded: the rollbacks restore what still runs.
            self.roll_back(progress).await;
            return Err(failed(
                JobErrorCode::RolledBack,
                format!("Rolled back before the mesh moved. {first}"),
            ));
        }
        // A beat past the instant, so that verify finds each node reloading or done.
        sleep_until(at + Duration::from_secs(1).min(lead)).await;
        self.move_hosts(progress).await?;
        Ok(format!(
            "Applied on {}; they reloaded together, so the mesh moved at once",
            nodes(applying.len())
        ))
    }
}
