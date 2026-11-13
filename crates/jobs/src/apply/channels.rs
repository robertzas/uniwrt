//! The Channels round (features/wifi.md, RF planner: Apply; M6.1 b1b, D-113): the nodes whose
//! radios' channels and widths alone change move after the fleet's transactions, one node at a
//! time, so their neighbors carry the clients while one moves.
//!
//! Each node's transaction is planned again from what it holds. Where every radio that moves
//! can announce the switch ([`super::air`]), the transaction installs `wireless` without
//! reloading it, and hostapd's `switch_chan` moves each radio with its clients; the switch has
//! taken once the kernel runs the new channel (the agent's `radios` set, from nl80211: hostapd
//! reports the new channel as soon as it announces it). A radio that refuses the switch or
//! doesn't get there in [`SWITCH_WAIT`] falls back to `wifi reload`, which restarts it on the
//! configuration (a stuck switch included). Elsewhere the transaction reloads. netifd and
//! hostapd learn of a switch only at the radio's next restart, on the channel it runs then.

use std::{fmt::Write as _, time::Duration};

use serde_json::Value;
use tokio::time::Instant;
use uniwrt_model::{
    apply::PlanAction,
    job::{JobError, JobErrorCode, JobNodeState},
};
use uniwrt_node::{
    ExecRequest,
    proto::ops::{Set, Ubus},
    txn::TxnId,
};

use super::{
    air::{Air, Move},
    round::Round,
    run::{Holds, Run, failed},
    survey,
};
use crate::{
    errors::{chain, internal},
    progress::Progress,
};

/// How long a radio may take to run the channel it announced: the announcement lasts
/// [`uniwrt_rf::switch::BEACONS`] beacons, about a second.
pub(crate) const SWITCH_WAIT: Duration = Duration::from_secs(10);

/// How long hostapd may take to answer `switch_chan`.
const CALL_TIMEOUT: Duration = Duration::from_secs(10);

impl Run<'_> {
    /// Moves the radios of the nodes `group`, one node after the other; the first failure
    /// rolls that node back and ends the round.
    pub(super) async fn channels(
        &mut self,
        group: &[usize],
        progress: &mut Progress,
    ) -> Result<String, JobError> {
        let (mut live, mut reloaded) = (0, 0);
        for &i in group {
            for live in &mut self.lives {
                live.active = false;
            }
            self.lives[i].active = true;
            let (id, name) = (self.lives[i].node.id, self.lives[i].node.name.clone());
            self.txn = TxnId::new(format!("{}-air-{}", self.base, id.get()))
                .map_err(|err| internal("naming the transaction", &err))?;
            let planned = match self.replan(i, Round::Air).await {
                Ok(()) => self.take_air(i),
                Err(why) => Err(why),
            };
            let air = match planned {
                Ok(Some(air)) => air,
                Ok(None) => continue,
                Err(why) => {
                    progress.node(id, JobNodeState::Failed, &why).await;
                    return Err(failed(JobErrorCode::NodeFailed, format!("{name}: {why}")));
                }
            };
            self.stage(progress).await?;
            self.apply(progress).await?;
            if self.lives[i].part.live.is_empty() {
                reloaded += 1;
            } else {
                live += 1;
                self.switch(i, &air, progress).await;
            }
            self.verify(progress).await?;
            self.confirm(progress).await?;
        }
        let mut done = Vec::new();
        if live > 0 {
            done.push(format!("{} by a channel switch", super::run::nodes(live)));
        }
        if reloaded > 0 {
            done.push(format!("{} by a reload", super::run::nodes(reloaded)));
        }
        Ok(if done.is_empty() {
            "Nothing left to move".to_owned()
        } else {
            format!(
                "Moved the channels of {}, one at a time",
                done.join(" and ")
            )
        })
    }

    /// Node `i`'s move as planned again: `None` when it holds the revision already; its
    /// transaction is the rendered `wireless`, live where every radio can switch and its power
    /// is in place.
    fn take_air(&mut self, i: usize) -> Result<Option<Air>, String> {
        let live = &mut self.lives[i];
        if live.part.plan.action != PlanAction::Change {
            live.holds = Holds::Revision;
            live.held = live.part.held.clone();
            live.active = false;
            return Ok(None);
        }
        let Some(air) = live.part.air.take() else {
            return Err("Its configuration changed under the apply".to_owned());
        };
        let file = air.wireless.to_file();
        live.part.held.insert("wireless".to_owned(), file.clone());
        live.part.files = vec![("wireless".to_owned(), file)];
        if air.first.is_none() && air.live() {
            live.part.live = air.moves.iter().map(|m| m.radio.clone()).collect();
        }
        Ok(Some(air))
    }

    /// Switches each radio of node `i`'s `air`, applied without a reload, and waits until the
    /// kernel runs it; reloads the radios that don't get there.
    async fn switch(&mut self, i: usize, air: &Air, progress: &mut Progress) {
        let live = &self.lives[i];
        let id = live.node.id;
        let Some(remote) = live.remote.as_deref() else {
            return;
        };
        let mut waiting: Vec<&Move> = Vec::new();
        let mut refused: Vec<String> = Vec::new();
        for m in &air.moves {
            let Ok((ifname, switch)) = &m.switch else {
                continue;
            };
            let mut call = Ubus::new(format!("hostapd.{ifname}"), "switch_chan");
            if let Ok(Value::Object(data)) = serde_json::to_value(switch) {
                call.data = data;
            }
            match crate::remote::request(remote, &call, CALL_TIMEOUT).await {
                Ok(_) => waiting.push(m),
                Err(err) => {
                    refused.push(format!("{} refused the switch: {}", m.radio, chain(&err)));
                }
            }
        }
        let wait = SWITCH_WAIT.min(self.shared.timing.verify);
        let until = Instant::now() + wait;
        while !waiting.is_empty() {
            if let Ok(facts) = survey::collect(self.shared, remote, &[Set::Radios], &[]).await {
                waiting.retain(|m| !runs(&facts.radios, m));
            }
            if waiting.is_empty() || Instant::now() >= until {
                break;
            }
            tokio::time::sleep(self.shared.timing.poll).await;
        }
        refused.extend(waiting.iter().map(|m| {
            format!(
                "{} didn't get to channel {} within {} s",
                m.radio,
                m.channel,
                wait.as_secs()
            )
        }));
        let detail = if refused.is_empty() {
            let moved: Vec<String> = air
                .moves
                .iter()
                .map(|m| {
                    format!(
                        "{} to channel {} at {} MHz",
                        m.radio,
                        m.channel,
                        m.width.mhz()
                    )
                })
                .collect();
            format!(
                "Switched {} with its clients; it rolls back by itself unless confirmed",
                moved.join(", ")
            )
        } else {
            let reload = remote.exec(ExecRequest::new(["wifi", "reload"])).await;
            let mut detail = format!("{}: reloaded its Wi-Fi instead", refused.join("; "));
            if let Err(err) = reload.and_then(|out| out.check("wifi").map(drop)) {
                let _ = write!(detail, " (the reload failed: {})", chain(&err));
            }
            detail
        };
        progress.node(id, JobNodeState::Applied, detail).await;
    }
}

/// Whether the kernel runs radio `m` where it moves: its primary and width.
fn runs(radios: &[uniwrt_model::facts::RadioFacts], m: &Move) -> bool {
    let Ok((_, switch)) = &m.switch else {
        return true;
    };
    radios.iter().any(|r| {
        r.name == m.radio && r.frequency_mhz == Some(switch.freq) && r.width == Some(m.width)
    })
}
