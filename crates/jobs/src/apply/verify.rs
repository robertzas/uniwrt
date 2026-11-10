//! The verify phase of an apply (features/lifecycle.md, Render → diff → apply, step 4): every
//! node at once over a new login at the address it moves to; it applied and reloaded, its
//! timer is extended, the bridging gateway sees this controller come from the new subnet
//! (D-081), and its health holds against the baseline. The first failure stops the others
//! and rolls everything back.

use std::{
    net::IpAddr,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use futures_util::future::join_all;
use tokio::time::{Instant, sleep};
use uniwrt_model::job::{JobError, JobErrorCode, JobNodeState};
use uniwrt_node::txn::{self, OpenState, TxnId};

use super::{
    controller,
    health::{self, Health},
    run::{Live, Run, failed, nodes},
    survey,
};
use crate::{errors::chain, progress::Progress, runner::Shared};

/// Why a node stopped verifying: another one failed.
const STOPPED: &str = "stopped: another node failed";

impl Run<'_> {
    /// Verifies every node at once; the first failure stops the others.
    pub(crate) async fn verify(&mut self, progress: &mut Progress) -> Result<String, JobError> {
        let stop = AtomicBool::new(false);
        let active = self.active();
        let pending = AtomicUsize::new(active.len());
        let (shared, txn) = (self.shared, &self.txn);
        let results = join_all(
            self.lives
                .iter_mut()
                .filter(|live| live.active)
                .map(|live| verify(shared, txn, live, &stop, &pending)),
        )
        .await;
        let mut first = None;
        for (live, result) in self.lives.iter().filter(|live| live.active).zip(&results) {
            let (state, detail) = match result {
                Ok(()) => (JobNodeState::Verified, format!("Healthy at {}", live.host)),
                Err(why) if why == STOPPED => continue,
                Err(why) => {
                    first.get_or_insert_with(|| format!("{}: {why}", live.node.name));
                    (JobNodeState::Failed, why.clone())
                }
            };
            progress.node(live.node.id, state, detail).await;
        }
        if let Some(first) = first {
            self.roll_back(progress).await;
            return Err(failed(
                JobErrorCode::RolledBack,
                format!("Rolled back. {first}"),
            ));
        }
        Ok(format!("{} healthy", nodes(active.len())))
    }
}

/// Verifies one node over a new login: it applied and reloaded, its timer is extended, and its
/// health holds against the baseline; then keeps its timer alive while the others verify.
async fn verify(
    shared: &Shared,
    txn: &TxnId,
    live: &mut Live,
    stop: &AtomicBool,
    pending: &AtomicUsize,
) -> Result<(), String> {
    let result = check(shared, txn, live, stop).await;
    if result.is_err() {
        stop.store(true, Ordering::SeqCst);
        return result;
    }
    pending.fetch_sub(1, Ordering::SeqCst);
    let poll = shared.timing.poll;
    while pending.load(Ordering::SeqCst) > 0 && !stop.load(Ordering::SeqCst) {
        sleep(poll).await;
        let timeout = live.part.timeout;
        if live.deadline.is_some_and(|deadline| {
            deadline.saturating_duration_since(Instant::now()) < timeout / 2
        }) && live.tool(&txn::extend(txn, timeout)).await.is_ok()
        {
            live.deadline = Some(Instant::now() + timeout);
        }
    }
    Ok(())
}

/// Waits until `live`'s transaction reports `applied`, logging in again at `candidates` when
/// the login drops.
pub(super) async fn wait_applied(
    shared: &Shared,
    txn: &TxnId,
    live: &mut Live,
    candidates: &[String],
    until: Instant,
) -> Result<(), String> {
    loop {
        match live.tool(&txn::status()).await {
            Ok(status) => {
                let status = status.ok_or_else(|| "The transaction tool is gone".to_owned())?;
                match &status.open {
                    Some(open) if open.id == txn.as_str() => match open.state {
                        OpenState::Applied => return Ok(()),
                        OpenState::RollingBack => return Err("It is rolling back".to_owned()),
                        _ => {}
                    },
                    _ => {
                        return Err(match status.last.filter(|last| last.id == txn.as_str()) {
                            Some(last) => format!(
                                "It rolled back by itself ({})",
                                last.reason.map_or("unknown", reason)
                            ),
                            None => "Its transaction isn't open: it never applied".to_owned(),
                        });
                    }
                }
            }
            Err(_) => {
                live.reconnect(shared, candidates).await;
            }
        }
        if Instant::now() >= until {
            return Err(format!(
                "Still reloading after {} s",
                until
                    .saturating_duration_since(Instant::now())
                    .as_secs()
                    .max(shared.timing.verify.as_secs())
            ));
        }
        sleep(shared.timing.poll).await;
    }
}

async fn check(
    shared: &Shared,
    txn: &TxnId,
    live: &mut Live,
    stop: &AtomicBool,
) -> Result<(), String> {
    let timing = &shared.timing;
    let window = if live.part.uplink {
        timing.verify_uplink
    } else {
        timing.verify
    };
    let until = Instant::now() + window;
    let stopped = || stop.load(Ordering::SeqCst);
    let late = || Instant::now() >= until;

    let candidates = live.candidates();
    loop {
        if stopped() {
            return Err(STOPPED.to_owned());
        }
        if live.reconnect(shared, &candidates).await {
            break;
        }
        if late() {
            return Err(format!(
                "Unreachable at {} since it applied",
                candidates.join(" or ")
            ));
        }
        sleep(timing.poll).await;
    }

    if !live.follows() {
        wait_applied(shared, txn, live, &candidates, until).await?;
        let timeout = live.part.timeout;
        live.tool(&txn::extend(txn, timeout))
            .await
            .map_err(|err| format!("Cannot extend its timer: {}", chain(&err)))?;
        live.deadline = Some(Instant::now() + timeout);
    }

    // The bridge holds until this controller comes from the new subnet: a new login each time,
    // since it is the controller's own address that changes.
    if let Some(bridge) = live.part.bridge.clone() {
        let subnet = bridge.new.trunc();
        loop {
            if stopped() {
                return Err(STOPPED.to_owned());
            }
            let login = survey::login(shared, &live.node, &live.host).await;
            let seen = if let Ok((_, remote)) = login {
                let seen = controller::read(remote.as_ref()).await;
                live.replace_remote(remote).await;
                seen
            } else {
                None
            };
            if let Some(IpAddr::V4(client)) = seen
                && subnet.contains(&client)
            {
                break;
            }
            if late() {
                let from = seen.map_or_else(|| "nowhere".to_owned(), |ip| ip.to_string());
                return Err(format!(
                    "This controller still reaches it from {from}, outside {subnet}, {} s after \
                     its ports bounced. Is this host a DHCP client wired to the gateway? Behind \
                     a switch, renew its lease while the apply verifies; a static address has \
                     to move by hand first",
                    window.as_secs()
                ));
            }
            sleep(timing.poll).await;
        }
    }

    loop {
        if stopped() {
            return Err(STOPPED.to_owned());
        }
        let problems = if let Some(remote) = live.remote.as_deref() {
            match survey::collect(shared, remote, &health::SETS, &health::SETS).await {
                Ok(facts) => {
                    let dns = survey::dns(remote, live.node.role).await;
                    Health::measure(live.node.role, &facts, dns).regressions(&live.baseline)
                }
                Err(why) => {
                    live.reconnect(shared, &candidates).await;
                    vec![why]
                }
            }
        } else {
            live.reconnect(shared, &candidates).await;
            vec!["Its login dropped".to_owned()]
        };
        if problems.is_empty() {
            return Ok(());
        }
        if late() {
            return Err(problems.join("; "));
        }
        sleep(timing.poll).await;
    }
}

fn reason(reason: txn::Reason) -> &'static str {
    match reason {
        txn::Reason::Timeout => "its timer ran out",
        txn::Reason::Boot => "it rebooted",
        txn::Reason::Requested => "the controller asked",
        txn::Reason::Invalid => "its configuration was invalid",
        txn::Reason::Unknown => "unknown",
    }
}
