//! The apply's backhaul round (features/mesh.md, Wired backhaul; D-094, D-097): each cable the
//! first round brought over as an overlay is probed from both ends, once batman-adv hears the
//! far end over the cable's VLAN; one that passes carries the mesh alone from then on, one that
//! fails goes back to a plain trunk and is remembered, so later applies leave it so. A probe that
//! can't be made (a login lost, an agent that doesn't answer) keeps the overlay for the next
//! apply to probe again. The transaction that follows is the round's, as for the gateway's
//! other later rounds ([`super::later`]).

use std::time::Duration;

use tokio::time::{Instant, sleep};
use tracing::warn;
use uniwrt_model::apply::{BackhaulFailure, CableEnd};
use uniwrt_node::proto::ops::{Mtu, Set};
use uniwrt_render::BackhaulStage;
use uniwrt_store::BackhaulRepo as _;

use super::{run::Run, survey};
use crate::{errors::chain, progress::Progress, remote::request};

/// How long the agent's probe may take: two pings and a reply each.
const PROBE_TIMEOUT: Duration = Duration::from_secs(30);

/// What probing a cable found.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Outcome {
    Passed,
    /// The cable can't carry batman-adv's frames: why, in words.
    Failed(String),
    /// The probe couldn't be made: why.
    Unknown(String),
}

impl Run<'_> {
    /// Probes every cable brought over as an overlay and sets each end's backhaul for the
    /// round's transaction: alone once it passed, gone once it failed, as it is otherwise.
    /// What it found, a sentence per cable.
    pub(super) async fn probe_cables(&mut self, progress: &Progress) -> Vec<String> {
        let mut words = Vec::new();
        for ends in std::mem::take(&mut self.probes) {
            let outcome = self.probe(&ends).await;
            let name = |end: &CableEnd| {
                self.lives
                    .iter()
                    .find(|live| live.node.id == end.node)
                    .map_or_else(|| end.node.to_string(), |live| live.node.name.clone())
            };
            let cable = format!(
                "{} {} to {} {}",
                name(&ends[0]),
                ends[0].port,
                name(&ends[1]),
                ends[1].port
            );
            let stage = match &outcome {
                Outcome::Passed => {
                    words.push(format!("{cable} carries the mesh alone"));
                    Some(BackhaulStage::Exclusive)
                }
                Outcome::Failed(why) => {
                    words.push(format!("{cable} goes back to a trunk: {why}"));
                    let failure = BackhaulFailure {
                        ends: ends.clone(),
                        job: progress.job_id(),
                        at: self.shared.clock.now(),
                        why: why.clone(),
                    };
                    if let Err(err) = self.shared.store.record_backhaul_failure(failure).await {
                        warn!("recording the wired backhaul's failure: {err}");
                    }
                    None
                }
                Outcome::Unknown(why) => {
                    words.push(format!(
                        "{cable} stays beside the networks until the next apply probes it: {why}"
                    ));
                    continue;
                }
            };
            for end in &ends {
                let Some(live) = self.lives.iter_mut().find(|l| l.node.id == end.node) else {
                    continue;
                };
                live.backhaul.retain(|b| b.port != end.port);
                if let Some(stage) = stage {
                    live.backhaul.push(uniwrt_render::Backhaul {
                        port: end.port.clone(),
                        stage,
                    });
                    live.backhaul.sort_by(|a, b| a.port.cmp(&b.port));
                }
            }
        }
        words
    }

    /// Probes one cable: batman-adv hears the far end over it at both ends, then the agent's
    /// MTU probe passes from both (a pass shows only what the peer receives).
    async fn probe(&self, ends: &[CableEnd; 2]) -> Outcome {
        let vid = self.site.mesh.wired_vlan;
        let mut at = Vec::new();
        for end in ends {
            match self.lives.iter().position(|live| live.node.id == end.node) {
                Some(i) => at.push(i),
                None => return Outcome::Unknown(format!("node {} isn't in the apply", end.node)),
            }
        }
        let until = Instant::now() + self.shared.timing.verify;
        for (&i, end) in at.iter().zip(ends) {
            let dev = format!("{}.{vid}", end.port);
            loop {
                match self.hears(i, &dev).await {
                    Ok(true) => break,
                    Ok(false) if Instant::now() < until => sleep(self.shared.timing.poll).await,
                    Ok(false) => {
                        return Outcome::Failed(format!(
                            "batman-adv on {}'s {} heard no node over the cable",
                            self.lives[i].node.name, end.port
                        ));
                    }
                    Err(why) => return Outcome::Unknown(why),
                }
            }
        }
        for (&i, end) in at.iter().zip(ends) {
            let live = &self.lives[i];
            let Some(remote) = live.remote.as_deref() else {
                return Outcome::Unknown(format!("{}'s login dropped", live.node.name));
            };
            let probe = Mtu::backhaul(&end.port, vid);
            match request(remote, &probe, PROBE_TIMEOUT).await {
                Ok(probed) if probed.passed() => {}
                Ok(probed) if probed.mtu < u32::from(probe.size) => {
                    return Outcome::Failed(format!(
                        "{}'s {} takes {} bytes, not {}",
                        live.node.name, end.port, probed.mtu, probe.size
                    ));
                }
                Ok(_) => {
                    return Outcome::Failed(format!(
                        "no node answered {} bytes sent from {}'s {}",
                        probe.size, live.node.name, end.port
                    ));
                }
                Err(err) => {
                    return Outcome::Unknown(format!(
                        "{} couldn't probe it: {}",
                        live.node.name,
                        chain(&err)
                    ));
                }
            }
        }
        Outcome::Passed
    }

    /// Whether node `i`'s batman-adv has a neighbor on the hard interface `dev`.
    async fn hears(&self, i: usize, dev: &str) -> Result<bool, String> {
        let live = &self.lives[i];
        let remote = live
            .remote
            .as_deref()
            .ok_or_else(|| format!("{}'s login dropped", live.node.name))?;
        let facts = survey::collect(self.shared, remote, &[Set::Batman], &[Set::Batman]).await?;
        let heard = facts
            .batman
            .iter()
            .any(|mesh| mesh.neighbors.iter().any(|n| n.hard_if == dev));
        Ok(heard)
    }
}
