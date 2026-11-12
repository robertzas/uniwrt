//! The transaction phases of an apply, after the survey (features/lifecycle.md, Render → diff →
//! apply, steps 3–5): stage and check on every node, apply leaves → gateway with the dead-man
//! timers armed, verify every node at once over new connections, then confirm all; any failure
//! before a confirm rolls every applied node back. The gateway may have transactions after the
//! first ([`Round`], D-081): dropping the old address its bridge kept once this controller
//! reaches it from the new subnet, and the WAN's change last; each is planned again from what it
//! holds then, and a failure there rolls back that transaction alone ([`super::later`]). The
//! verify phase is [`super::verify`].

use std::{collections::BTreeMap, fmt::Write as _, time::Duration};

use serde::de::DeserializeOwned;
use tokio::time::Instant;
use uniwrt_model::{
    apply::{CableEnd, PlanAction},
    job::{JobError, JobErrorCode, JobNodeState},
    node::Node,
    site::SiteConfig,
};
use uniwrt_node::{
    HostKey,
    txn::{self, TxnId, code},
};
use uniwrt_render::Backhaul;

use super::{
    health::Health,
    plan::Part,
    round::Round,
    survey::{self, Member, Reading, VaultSecrets},
};
use crate::{Remote, errors::chain, progress::Progress, runner::Shared};

/// How much of the revision a node holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Holds {
    /// What it held before the apply.
    Before,
    /// Its first transactions are confirmed, a later one isn't (D-081).
    Part,
    /// All of it: its last transaction is confirmed, or it had none.
    Revision,
}

/// A node that changes, as the apply carries it out.
pub(crate) struct Live {
    pub(crate) node: Node,
    pub(crate) host_key: HostKey,
    /// The login the next command goes over; `None` once lost.
    pub(super) remote: Option<Box<dyn Remote>>,
    pub(crate) part: Part,
    pub(crate) reading: Reading,
    pub(super) baseline: Health,
    /// When its timer rolls it back, once applied.
    pub(super) deadline: Option<Instant>,
    /// Its current transaction is confirmed.
    pub(crate) confirmed: bool,
    /// Where it answered last.
    pub(crate) host: String,
    /// It takes part in the current round.
    pub(super) active: bool,
    /// It was asked to take its new address while the gateway hands it out: only that one
    /// answers for it now.
    pub(super) moves: bool,
    /// How much of the revision its confirmed transactions gave it.
    pub(crate) holds: Holds,
    /// Its managed packages as it holds them after its last confirmed transaction.
    pub(crate) held: BTreeMap<String, String>,
    /// The cables batman-adv runs over, at the stage its part brings them to.
    pub(crate) backhaul: Vec<Backhaul>,
}

impl From<Member> for Live {
    fn from(member: Member) -> Self {
        // A node with nothing to change before the backhaul round sits the first rounds out.
        let first = member.part.plan.action == PlanAction::Change || member.part.follows();
        Self {
            host: member.login.node.host.clone(),
            node: member.login.node,
            host_key: member.login.host_key,
            remote: Some(member.login.remote),
            reading: member.reading,
            baseline: member.baseline,
            deadline: None,
            confirmed: false,
            active: first,
            moves: false,
            holds: if first { Holds::Before } else { Holds::Part },
            held: if first {
                BTreeMap::new()
            } else {
                member.part.held.clone()
            },
            backhaul: member.backhaul,
            part: member.part,
        }
    }
}

impl Live {
    /// Runs a command of the tool over the current login.
    pub(super) async fn tool<T: DeserializeOwned>(
        &self,
        command: &txn::Command<T>,
    ) -> uniwrt_node::Result<T> {
        let remote = self
            .remote
            .as_ref()
            .ok_or_else(|| uniwrt_node::Error::Disconnected {
                what: "the apply's login".to_owned(),
            })?;
        let output = remote.exec(command.request.clone()).await?;
        command.parse(output)
    }

    /// The feed packages to install before its transaction.
    pub(crate) fn installs(
        &self,
    ) -> Option<&(
        uniwrt_model::facts::PackageManager,
        Vec<uniwrt_render::Requirement>,
    )> {
        self.part.installs.as_ref()
    }

    /// Runs a command over the current login.
    pub(crate) async fn exec(
        &self,
        request: uniwrt_node::ExecRequest,
    ) -> uniwrt_node::Result<uniwrt_node::ExecOutput> {
        let remote = self
            .remote
            .as_ref()
            .ok_or_else(|| uniwrt_node::Error::Disconnected {
                what: "the apply's login".to_owned(),
            })?;
        remote.exec(request).await
    }

    /// Nothing to apply this round, but an address to follow.
    pub(super) fn follows(&self) -> bool {
        self.part.follows()
    }

    /// The addresses to try, the one it moves to first, then where it answered last; only the
    /// new one while it bridges (the old one goes next) or was asked to move (verified at the
    /// old one, it would be lost once it moves).
    pub(super) fn candidates(&self) -> Vec<String> {
        let hosts: Vec<String> = self.part.address.iter().map(ToString::to_string).collect();
        if (self.part.bridge.is_some() || self.moves) && !hosts.is_empty() {
            return hosts;
        }
        self.anywhere()
    }

    /// Every address it may answer at: the one it moves to, where it answered last, and where
    /// it was before the apply (a rollback returns it there).
    fn anywhere(&self) -> Vec<String> {
        let mut hosts: Vec<String> = self.part.address.iter().map(ToString::to_string).collect();
        for host in [&self.host, &self.node.host] {
            if !hosts.contains(host) {
                hosts.push(host.clone());
            }
        }
        hosts
    }

    pub(super) async fn replace_remote(&mut self, remote: Box<dyn Remote>) {
        if let Some(old) = self.remote.replace(remote) {
            old.close().await;
        }
    }

    /// Logs in again at the first of `candidates` that answers.
    pub(super) async fn reconnect(&mut self, shared: &Shared, candidates: &[String]) -> bool {
        for host in candidates {
            if let Ok((_, remote)) = survey::login(shared, &self.node, host).await {
                self.replace_remote(remote).await;
                self.host.clone_from(host);
                return true;
            }
        }
        false
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
    pub(super) site: &'a SiteConfig,
    pub(super) secrets: VaultSecrets,
    /// The current round's transaction.
    pub(crate) txn: TxnId,
    /// The first round's id, which the later rounds' derive from.
    pub(super) base: String,
    pub(crate) lives: Vec<Live>,
    /// The cables the backhaul round probes.
    pub(super) probes: Vec<[CableEnd; 2]>,
}

pub(super) fn failed(code: JobErrorCode, message: String) -> JobError {
    JobError { code, message }
}

impl<'a> Run<'a> {
    pub(crate) fn new(
        shared: &'a Shared,
        site: &'a SiteConfig,
        secrets: VaultSecrets,
        txn: TxnId,
        lives: Vec<Live>,
        probes: Vec<[CableEnd; 2]>,
    ) -> Self {
        Self {
            shared,
            site,
            secrets,
            base: txn.as_str().to_owned(),
            txn,
            lives,
            probes,
        }
    }

    /// The nodes in the current round.
    pub(super) fn active(&self) -> Vec<usize> {
        (0..self.lives.len())
            .filter(|&i| self.lives[i].active)
            .collect()
    }

    /// Stages and checks every transaction; nothing changes yet.
    pub(crate) async fn stage(&mut self, progress: &mut Progress) -> Result<String, JobError> {
        let active = self.active();
        for &i in &active {
            let live = &self.lives[i];
            let id = live.node.id;
            if live.follows() {
                let detail = format!(
                    "Nothing to change; it follows to {} once the gateway renumbers",
                    live.host_to()
                );
                progress.node(id, JobNodeState::Staged, detail).await;
                continue;
            }
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
        Ok(format!("Staged and checked on {}", nodes(active.len())))
    }

    /// Applies every transaction in order, keeping the earlier timers alive; then the access
    /// points that move ask for a lease, and the gateway's ports bounce where it bridges.
    pub(crate) async fn apply(&mut self, progress: &mut Progress) -> Result<String, JobError> {
        let active = self.active();
        for &i in &active {
            let id = self.lives[i].node.id;
            if self.lives[i].follows() {
                progress
                    .node(
                        id,
                        JobNodeState::Applied,
                        "Follows once the gateway applied",
                    )
                    .await;
                continue;
            }
            let timeout = self.lives[i].part.timeout;
            let result = self.lives[i].tool(&txn::apply(&self.txn, timeout)).await;
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
        self.move_hosts(progress).await?;
        Ok(format!("Applied on {}", nodes(active.len())))
    }

    /// Extends the timers of the nodes before `i` that have less than half theirs left.
    async fn keep_alive(&self, i: usize) {
        for live in self.lives[..i].iter().filter(|live| live.active) {
            let (Some(deadline), timeout) = (live.deadline, live.part.timeout) else {
                continue;
            };
            if deadline.saturating_duration_since(Instant::now()) < timeout / 2 {
                // Best effort: verifying extends it again over a new login.
                let _ = live.tool(&txn::extend(&self.txn, timeout)).await;
            }
        }
    }

    /// Confirms every transaction in order. A first confirm that fails rolls the rest back; a
    /// later one leaves the apply partial, and the rest still confirm.
    pub(crate) async fn confirm(&mut self, progress: &mut Progress) -> Result<String, JobError> {
        let mut lost = Vec::new();
        let active = self.active();
        for &i in &active {
            let live = &self.lives[i];
            let id = live.node.id;
            if live.follows() {
                let live = &mut self.lives[i];
                live.confirmed = true;
                live.holds = Holds::Revision;
                live.held = live.part.held.clone();
                let detail = if live.host == live.node.host {
                    format!("Stays at {}", live.host)
                } else {
                    format!("Moved to {}", live.host)
                };
                progress.node(id, JobNodeState::Confirmed, detail).await;
                continue;
            }
            match live.tool(&txn::confirm(&self.txn)).await {
                Ok(_) => {
                    let live = &mut self.lives[i];
                    live.confirmed = true;
                    live.holds = if live.part.next.is_empty() {
                        Holds::Revision
                    } else {
                        Holds::Part
                    };
                    live.held = live.part.held.clone();
                    live.reading.confirmed(&live.part);
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
                    let name = live.node.name.clone();
                    progress.node(id, JobNodeState::Failed, &why).await;
                    if !active.iter().any(|&j| self.lives[j].confirmed) {
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
            for &i in &active {
                if !self.lives[i].confirmed {
                    let _ = self.lives[i].tool(&txn::rollback(&self.txn)).await;
                }
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
        Ok(format!("Confirmed on {}", nodes(active.len())))
    }

    /// Rolls back every applied node of the round, the gateway first, and marks the others not
    /// applied.
    pub(super) async fn roll_back(&mut self, progress: &mut Progress) {
        for i in self.active().into_iter().rev() {
            let live = &self.lives[i];
            if live.deadline.is_none() || live.confirmed {
                continue;
            }
            let mut done = rolled_back(live.tool(&txn::rollback(&self.txn)).await);
            if !done {
                let candidates = live.anywhere();
                if self.lives[i].reconnect(self.shared, &candidates).await {
                    done = rolled_back(self.lives[i].tool(&txn::rollback(&self.txn)).await);
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

    /// Marks the nodes of the round that never applied.
    pub(crate) async fn not_applied(&self, progress: &mut Progress) {
        for live in self
            .lives
            .iter()
            .filter(|live| live.active && live.deadline.is_none() && !live.confirmed)
        {
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

impl Live {
    /// Where it is going, in words.
    fn host_to(&self) -> String {
        self.part
            .address
            .map_or_else(|| self.node.host.clone(), |addr| addr.to_string())
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
    let mut text = format!("Staged and checked: {}", what.join("; "));
    if let Some(bridge) = &part.bridge {
        let _ = write!(
            text,
            "; keeps {} beside {} until this controller comes from {}",
            bridge.old,
            bridge.new,
            bridge.new.trunc()
        );
    }
    if part.next.contains(&Round::Wan) {
        text.push_str("; the WAN's change goes last");
    }
    if part.next.contains(&Round::Backhaul) {
        text.push_str("; its cables to other nodes are probed next");
    }
    text
}

/// "1 node", "3 nodes".
pub(crate) fn nodes(n: usize) -> String {
    if n == 1 {
        "1 node".to_owned()
    } else {
        format!("{n} nodes")
    }
}
