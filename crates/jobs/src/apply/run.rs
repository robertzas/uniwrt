//! The transaction phases of an apply, after the survey (features/lifecycle.md, Render → diff →
//! apply, steps 3–5): stage and check on every node, apply leaves → gateway with the dead-man
//! timers armed, verify every node at once over new connections, then confirm all; any failure
//! before a confirm rolls every applied node back.

use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    time::Duration,
};

use futures_util::future::join_all;
use serde::de::DeserializeOwned;
use tokio::time::{Instant, sleep};
use uniwrt_model::{
    job::{JobError, JobErrorCode, JobNodeState, StepName},
    node::Node,
};
use uniwrt_node::{
    HostKey,
    txn::{self, OpenState, TxnId, code},
};

use super::{
    health::{self, Health},
    plan::Part,
    survey::{self, Member},
};
use crate::{Remote, errors::chain, progress::Progress, runner::Shared};

/// Why a node stopped verifying: another one failed.
const STOPPED: &str = "stopped: another node failed";

/// A node that changes, as the apply carries it out.
pub(crate) struct Live {
    pub(crate) node: Node,
    pub(crate) host_key: HostKey,
    /// The login the next command goes over; `None` once lost.
    remote: Option<Box<dyn Remote>>,
    part: Part,
    baseline: Health,
    /// When its timer rolls it back, once applied.
    deadline: Option<Instant>,
    pub(crate) confirmed: bool,
    /// Where it answered last.
    pub(crate) host: String,
}

impl From<Member> for Live {
    fn from(member: Member) -> Self {
        Self {
            host: member.login.node.host.clone(),
            node: member.login.node,
            host_key: member.login.host_key,
            remote: Some(member.login.remote),
            part: member.part,
            baseline: member.baseline,
            deadline: None,
            confirmed: false,
        }
    }
}

impl Live {
    /// Its managed packages as it holds them once applied.
    pub(crate) fn held(&self) -> &BTreeMap<String, String> {
        &self.part.held
    }

    /// Runs a command of the tool over the current login.
    async fn tool<T: DeserializeOwned>(&self, command: &txn::Command<T>) -> uniwrt_node::Result<T> {
        let remote = self
            .remote
            .as_ref()
            .ok_or_else(|| uniwrt_node::Error::Disconnected {
                what: "the apply's login".to_owned(),
            })?;
        let output = remote.exec(command.request.clone()).await?;
        command.parse(output)
    }

    /// The addresses to try, the one it moves to first.
    fn candidates(&self) -> Vec<String> {
        let mut hosts: Vec<String> = self.part.address.iter().map(ToString::to_string).collect();
        if !hosts.contains(&self.node.host) {
            hosts.push(self.node.host.clone());
        }
        hosts
    }

    async fn replace_remote(&mut self, remote: Box<dyn Remote>) {
        if let Some(old) = self.remote.replace(remote) {
            old.close().await;
        }
    }

    pub(crate) async fn close(&mut self) {
        if let Some(remote) = self.remote.take() {
            remote.close().await;
        }
    }
}

/// The phases' shared context.
pub(crate) struct Run<'a> {
    pub(crate) shared: &'a Shared,
    pub(crate) txn: TxnId,
    pub(crate) lives: Vec<Live>,
}

fn failed(code: JobErrorCode, message: String) -> JobError {
    JobError { code, message }
}

impl Run<'_> {
    /// Stages and checks every transaction; nothing changes yet.
    pub(crate) async fn stage(&mut self, progress: &mut Progress) -> Result<(), JobError> {
        progress.start(StepName::Stage).await;
        for i in 0..self.lives.len() {
            let live = &self.lives[i];
            let result = async {
                let services = live
                    .part
                    .services
                    .iter()
                    .map(|(name, on)| (name.as_str(), *on));
                live.tool(&txn::stage(&self.txn, services)?).await?;
                for (package, file) in &live.part.files {
                    live.tool(&txn::put(&self.txn, package, file.clone())?)
                        .await?;
                }
                live.tool(&txn::check(&self.txn)).await
            }
            .await;
            let id = live.node.id;
            if let Err(err) = result {
                let why = format!("It refused the configuration: {}", chain(&err));
                progress.node(id, JobNodeState::Failed, &why).await;
                self.not_applied(progress).await;
                return Err(failed(
                    JobErrorCode::NodeFailed,
                    format!("Nothing changed. {}: {why}", live.node.name),
                ));
            }
            let staged = staged(&live.part);
            progress.node(id, JobNodeState::Staged, staged).await;
        }
        progress
            .done(Some(format!(
                "Staged and checked on {}",
                nodes(self.lives.len())
            )))
            .await;
        Ok(())
    }

    /// Applies every transaction in order, keeping the earlier timers alive.
    pub(crate) async fn apply(&mut self, progress: &mut Progress) -> Result<(), JobError> {
        progress.start(StepName::Apply).await;
        for i in 0..self.lives.len() {
            let timeout = self.lives[i].part.timeout;
            let result = self.lives[i].tool(&txn::apply(&self.txn, timeout)).await;
            let id = self.lives[i].node.id;
            match result {
                Ok(_) => {
                    self.lives[i].deadline = Some(Instant::now() + timeout);
                    let detail = format!(
                        "Applied; it rolls back by itself in {} s unless confirmed",
                        timeout.as_secs()
                    );
                    progress.node(id, JobNodeState::Applied, detail).await;
                }
                Err(uniwrt_node::Error::Txn { msg, .. }) => {
                    // The tool refused it, or rolled it back itself (an invalid firewall).
                    let why = format!("It could not apply: {msg}");
                    progress.node(id, JobNodeState::Failed, &why).await;
                    let name = self.lives[i].node.name.clone();
                    self.roll_back(progress).await;
                    return Err(failed(
                        JobErrorCode::RolledBack,
                        format!("Rolled back. {name}: {why}"),
                    ));
                }
                Err(err) => {
                    // The answer is lost, not necessarily the apply: verifying tells.
                    self.lives[i].deadline = Some(Instant::now() + timeout);
                    self.lives[i].close().await;
                    let detail = format!("The connection dropped as it applied: {}", chain(&err));
                    progress.node(id, JobNodeState::Applied, detail).await;
                }
            }
            self.keep_alive(i).await;
        }
        progress
            .done(Some(format!("Applied on {}", nodes(self.lives.len()))))
            .await;
        Ok(())
    }

    /// Extends the timers of the nodes before `i` that have less than half theirs left.
    async fn keep_alive(&self, i: usize) {
        for live in &self.lives[..i] {
            let (Some(deadline), timeout) = (live.deadline, live.part.timeout) else {
                continue;
            };
            if deadline.saturating_duration_since(Instant::now()) < timeout / 2 {
                // Best effort: verifying extends it again over a new login.
                let _ = live.tool(&txn::extend(&self.txn, timeout)).await;
            }
        }
    }

    /// Verifies every node at once; the first failure stops the others.
    pub(crate) async fn verify(&mut self, progress: &mut Progress) -> Result<(), JobError> {
        progress.start(StepName::Verify).await;
        let stop = AtomicBool::new(false);
        let pending = AtomicUsize::new(self.lives.len());
        let (shared, txn) = (self.shared, &self.txn);
        let results = join_all(
            self.lives
                .iter_mut()
                .map(|live| verify(shared, txn, live, &stop, &pending)),
        )
        .await;
        let mut first = None;
        for (live, result) in self.lives.iter().zip(&results) {
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
        progress
            .done(Some(format!("{} healthy", nodes(self.lives.len()))))
            .await;
        Ok(())
    }

    /// Confirms every transaction in order. A first confirm that fails rolls the rest back; a
    /// later one leaves the apply partial, and the rest still confirm.
    pub(crate) async fn confirm(&mut self, progress: &mut Progress) -> Result<(), JobError> {
        progress.start(StepName::Confirm).await;
        let mut lost = Vec::new();
        for i in 0..self.lives.len() {
            let live = &self.lives[i];
            match live.tool(&txn::confirm(&self.txn)).await {
                Ok(_) => {
                    self.lives[i].confirmed = true;
                    let id = self.lives[i].node.id;
                    progress
                        .node(id, JobNodeState::Confirmed, "Confirmed")
                        .await;
                }
                Err(err) => {
                    let why = match &err {
                        uniwrt_node::Error::Txn { code: c, .. } if c == code::EXPIRED => {
                            "It rolled back by itself before the confirm".to_owned()
                        }
                        _ => format!("The confirm failed: {}", chain(&err)),
                    };
                    let (id, name) = (live.node.id, live.node.name.clone());
                    progress.node(id, JobNodeState::Failed, &why).await;
                    if !self.lives.iter().any(|live| live.confirmed) {
                        self.roll_back(progress).await;
                        return Err(failed(
                            JobErrorCode::RolledBack,
                            format!("Rolled back. {name}: {why}"),
                        ));
                    }
                    lost.push(format!("{name}: {why}"));
                }
            }
        }
        if !lost.is_empty() {
            // A failed node rolls back by itself at its deadline, if it hasn't already.
            for live in self.lives.iter_mut().filter(|live| !live.confirmed) {
                let _ = live.tool(&txn::rollback(&self.txn)).await;
            }
            return Err(failed(
                JobErrorCode::PartialApply,
                format!(
                    "Partly applied: the confirmed nodes hold the revision, the others what they \
                     had. Keep them, or apply the previous revision. {}",
                    lost.join(". ")
                ),
            ));
        }
        progress
            .done(Some(format!("Confirmed on {}", nodes(self.lives.len()))))
            .await;
        Ok(())
    }

    /// Rolls back every applied node, the gateway first, and marks the others not applied.
    async fn roll_back(&mut self, progress: &mut Progress) {
        for i in (0..self.lives.len()).rev() {
            let live = &self.lives[i];
            if live.deadline.is_none() || live.confirmed {
                continue;
            }
            let mut done = rolled_back(live.tool(&txn::rollback(&self.txn)).await);
            if !done {
                let (node, candidates) = (live.node.clone(), live.candidates());
                for host in candidates {
                    if let Ok((_, remote)) = survey::login(self.shared, &node, &host).await {
                        self.lives[i].replace_remote(remote).await;
                        done = rolled_back(self.lives[i].tool(&txn::rollback(&self.txn)).await);
                        break;
                    }
                }
            }
            let live = &self.lives[i];
            let detail = if done {
                "Rolled back".to_owned()
            } else {
                let left = live.deadline.map_or(Duration::ZERO, |deadline| {
                    deadline.saturating_duration_since(Instant::now())
                });
                format!(
                    "Unreachable: it rolls back by itself within {} s",
                    left.as_secs()
                )
            };
            progress
                .node(live.node.id, JobNodeState::RolledBack, detail)
                .await;
        }
        self.not_applied(progress).await;
    }

    /// Marks the nodes that never applied.
    async fn not_applied(&self, progress: &mut Progress) {
        for live in self.lives.iter().filter(|live| live.deadline.is_none()) {
            if progress.node_state(live.node.id) != Some(JobNodeState::Failed) {
                progress
                    .node(live.node.id, JobNodeState::RolledBack, "Not applied")
                    .await;
            }
        }
    }

    pub(crate) async fn close(&mut self) {
        for live in &mut self.lives {
            live.close().await;
        }
    }
}

/// Whether a rollback answer means the node holds what it had: it rolled back now, earlier,
/// or never applied.
fn rolled_back(result: uniwrt_node::Result<txn::Done>) -> bool {
    match result {
        Ok(_) => true,
        Err(uniwrt_node::Error::Txn { code: c, .. }) => c == code::EXPIRED || c == code::NOT_FOUND,
        Err(_) => false,
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
    'reach: loop {
        if stopped() {
            return Err(STOPPED.to_owned());
        }
        for host in &candidates {
            if let Ok((_, remote)) = survey::login(shared, &live.node, host).await {
                live.replace_remote(remote).await;
                live.host.clone_from(host);
                break 'reach;
            }
        }
        if late() {
            return Err(format!(
                "Unreachable at {} since it applied",
                candidates.join(" or ")
            ));
        }
        sleep(timing.poll).await;
    }

    loop {
        if stopped() {
            return Err(STOPPED.to_owned());
        }
        if let Ok(status) = live.tool(&txn::status()).await {
            let status = status.ok_or_else(|| "The transaction tool is gone".to_owned())?;
            match &status.open {
                Some(open) if open.id == txn.as_str() => match open.state {
                    OpenState::Applied => break,
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
        if late() {
            return Err(format!("Still reloading after {} s", window.as_secs()));
        }
        sleep(timing.poll).await;
    }

    let timeout = live.part.timeout;
    live.tool(&txn::extend(txn, timeout))
        .await
        .map_err(|err| format!("Cannot extend its timer: {}", chain(&err)))?;
    live.deadline = Some(Instant::now() + timeout);

    loop {
        if stopped() {
            return Err(STOPPED.to_owned());
        }
        let remote = live
            .remote
            .as_deref()
            .ok_or_else(|| "Its login dropped".to_owned())?;
        let problems = match survey::collect(shared, remote, &health::SETS, &health::SETS).await {
            Ok(facts) => {
                let dns = survey::dns(remote, live.node.role).await;
                Health::measure(live.node.role, &facts, dns).regressions(&live.baseline)
            }
            Err(why) => vec![why],
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

fn staged(part: &Part) -> String {
    let packages: Vec<&str> = part.files.iter().map(|(name, _)| name.as_str()).collect();
    let services: Vec<String> = part
        .services
        .iter()
        .map(|(name, on)| format!("{name} {}", if *on { "on" } else { "off" }))
        .collect();
    let mut what = Vec::new();
    if !packages.is_empty() {
        what.push(packages.join(", "));
    }
    if !services.is_empty() {
        what.push(services.join(", "));
    }
    format!("Staged and checked: {}", what.join("; "))
}

/// "1 node", "3 nodes".
pub(crate) fn nodes(n: usize) -> String {
    if n == 1 {
        "1 node".to_owned()
    } else {
        format!("{n} nodes")
    }
}
