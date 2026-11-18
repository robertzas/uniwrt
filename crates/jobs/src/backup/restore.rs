//! Restoring a backup's configuration onto its node (features/lifecycle.md, Backups; D-126): the
//! UCI packages the backup holds (`/etc/config/*`) go back through the transaction tool, so a
//! node that doesn't come back as healthy rolls back by itself. The backup's other files
//! (passwords, host keys, scripts) are not restored: its archive can be downloaded for
//! `sysupgrade -r`.
//!
//! connect → check (managed, no open transaction; the archive's packages parse; those that
//! differ from the node's; the health baseline) → backup (what it holds now, reason `restore`)
//! → stage (the packages that differ, checked by `uci`) → apply (the dead-man timer of an uplink
//! change) → verify (a new login at its address, as healthy as before within the uplink's
//! verify time; else rolled back) → confirm.

use std::{collections::BTreeMap, sync::Arc};

use tokio::time::Instant;
use tracing::{info, warn};
use uniwrt_core::{BackupId, UserId};
use uniwrt_model::{
    facts::NodeFacts,
    inventory::{Backup, BackupReason},
    job::{JobError, JobErrorCode, JobKind, NewJob, StepName},
    node::Node,
};
use uniwrt_node::{
    archive,
    proto::ops::Set,
    txn::{self, TxnId, current},
};

use super::take;
use crate::{
    Remote,
    apply::{health::Health, survey},
    errors::{chain, internal, txn_error},
    manage,
    progress::Progress,
    runner::Shared,
};

/// The facts the health checks read.
const SETS: [Set; 5] = [
    Set::Board,
    Set::Interfaces,
    Set::Radios,
    Set::Batman,
    Set::Stations,
];

const STEPS: [StepName; 7] = [
    StepName::Connect,
    StepName::Check,
    StepName::Backup,
    StepName::Stage,
    StepName::Apply,
    StepName::Verify,
    StepName::Confirm,
];

pub(crate) fn job(node: &Node, backup: &Backup, by: Option<UserId>) -> NewJob {
    NewJob {
        kind: JobKind::Restore,
        target: format!("{}, backup of {}", node.name, backup.taken_at),
        node: Some(node.id),
        revision: None,
        created_by: by,
        steps: STEPS.to_vec(),
        nodes: Vec::new(),
    }
}

pub(crate) async fn run(shared: Arc<Shared>, node: Node, backup: BackupId, mut progress: Progress) {
    let result = restore(&shared, &node, backup, &mut progress).await;
    match &result {
        Ok(()) => info!(node = %node.id, %backup, "backup restored"),
        Err(err) => warn!(node = %node.id, code = ?err.code, "{}", err.message),
    }
    progress.finish(result.err()).await;
}

fn refused(message: impl Into<String>) -> JobError {
    JobError {
        code: JobErrorCode::Refused,
        message: message.into(),
    }
}

/// The packages to put back, and how healthy the node is before.
struct Checked {
    changed: BTreeMap<String, String>,
    baseline: Health,
}

async fn restore(
    shared: &Shared,
    node: &Node,
    backup: BackupId,
    progress: &mut Progress,
) -> Result<(), JobError> {
    let remote = manage::connect(shared, node, progress).await?;
    let checked = check(shared, node, backup, remote.as_ref(), progress).await;
    let checked = match checked {
        Ok(Some(checked)) => checked,
        Ok(None) => {
            remote.close().await;
            for step in &STEPS[2..] {
                progress
                    .skip(*step, "It holds the backup's configuration already")
                    .await;
            }
            return Ok(());
        }
        Err(err) => {
            remote.close().await;
            return Err(err);
        }
    };
    let txn = TxnId::new(format!("uniwrt-{}", progress.job_id().get()))
        .map_err(|err| internal("naming the transaction", &err))?;
    let staged = async {
        progress.start(StepName::Backup).await;
        let kept = take(shared, node.id, remote.as_ref(), BackupReason::Restore).await?;
        progress.done(Some(kept)).await;
        stage(remote.as_ref(), &txn, &checked.changed, progress).await
    }
    .await;
    if let Err(err) = staged {
        remote.close().await;
        return Err(err);
    }
    progress.start(StepName::Apply).await;
    let timer = shared.timing.timer_uplink;
    let command = txn::apply(&txn, timer);
    let applied = remote
        .exec(command.request.clone())
        .await
        .and_then(|output| command.parse(output));
    remote.close().await;
    // Applied, or the answer is lost with the reload: the verify tells.
    if let Err(err @ uniwrt_node::Error::Txn { .. }) = applied {
        return Err(txn_error(&err, "It could not apply the configuration"));
    }
    progress
        .done(Some(format!(
            "{}: it rolls back by itself unless confirmed within {} s",
            checked
                .changed
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(", "),
            timer.as_secs()
        )))
        .await;
    let remote = verify(shared, node, &checked.baseline, &txn, progress).await?;
    progress.start(StepName::Confirm).await;
    let command = txn::confirm(&txn);
    let confirmed = remote
        .exec(command.request.clone())
        .await
        .and_then(|output| command.parse(output));
    remote.close().await;
    confirmed.map_err(|err| txn_error(&err, "Confirming the restored configuration"))?;
    progress
        .done(Some(
            "It holds the backup's configuration; the site's revision differs from it until the \
             next apply"
                .to_owned(),
        ))
        .await;
    Ok(())
}

/// The Check step: `None` when the node holds the backup's packages already.
async fn check(
    shared: &Shared,
    node: &Node,
    backup: BackupId,
    remote: &dyn Remote,
    progress: &mut Progress,
) -> Result<Option<Checked>, JobError> {
    progress.start(StepName::Check).await;
    if !survey::idle_tool(remote).await.map_err(refused)? {
        return Err(refused(
            "It lacks the transaction tool: enable management first",
        ));
    }
    let (found, tarball) = shared
        .vault
        .open_backup(backup)
        .await
        .map_err(|err| internal("opening the backup", &err))?
        .ok_or_else(|| refused("The backup is gone"))?;
    if found.node != node.id {
        return Err(refused("The backup is another node's"));
    }
    let entries = archive::entries(&tarball).map_err(|err| refused(err.to_string()))?;
    let mut packages = BTreeMap::new();
    for (name, bytes) in archive::config_files(&entries) {
        let text = String::from_utf8(bytes.to_vec())
            .map_err(|_| refused(format!("Its {name} isn't UTF-8 text")))?;
        uniwrt_uci::parse(&name, &text)
            .map_err(|err| refused(format!("Its {name} doesn't parse: {err}")))?;
        packages.insert(name, text);
    }
    if packages.is_empty() {
        return Err(refused("The backup holds no /etc/config package"));
    }
    let count = packages.len();
    let names: Vec<&str> = packages.keys().map(String::as_str).collect();
    let read = current::packages(&names).map_err(|err| refused(err.to_string()))?;
    let held = remote
        .exec(read.request.clone())
        .await
        .and_then(|output| read.parse(output))
        .map_err(|err| refused(format!("Cannot read its configuration: {}", chain(&err))))?;
    let changed: BTreeMap<String, String> = packages
        .into_iter()
        .filter(|(name, text)| held.get(name).and_then(Option::as_ref) != Some(text))
        .collect();
    if changed.is_empty() {
        progress
            .done(Some(
                "It holds the backup's configuration already".to_owned(),
            ))
            .await;
        return Ok(None);
    }
    let facts = survey::collect(shared, remote, &SETS, &[Set::Board])
        .await
        .map_err(refused)?;
    let baseline = Health::measure(node.role, &facts, survey::dns(remote, node.role).await);
    progress
        .done(Some(format!(
            "{} of the backup's {} packages differ: {}",
            changed.len(),
            count,
            changed.keys().cloned().collect::<Vec<_>>().join(", ")
        )))
        .await;
    Ok(Some(Checked { changed, baseline }))
}

/// The Stage step: the packages staged and checked by `uci`.
async fn stage(
    remote: &dyn Remote,
    txn: &TxnId,
    changed: &BTreeMap<String, String>,
    progress: &mut Progress,
) -> Result<(), JobError> {
    progress.start(StepName::Stage).await;
    let staged = async {
        let command = txn::stage(txn, [])?;
        command.parse(remote.exec(command.request.clone()).await?)?;
        for (name, text) in changed {
            let command = txn::put(txn, name, text.clone())?;
            command.parse(remote.exec(command.request.clone()).await?)?;
        }
        let command = txn::check(txn);
        command.parse(remote.exec(command.request.clone()).await?)
    }
    .await;
    staged.map_err(|err| txn_error(&err, "The node refused the backup's configuration"))?;
    progress
        .done(Some("Staged and checked; nothing changed yet".to_owned()))
        .await;
    Ok(())
}

/// The Verify step: a new login, and the health of before within the uplink's verify time;
/// else the transaction rolls back. Returns the login to confirm over.
async fn verify(
    shared: &Shared,
    node: &Node,
    baseline: &Health,
    txn: &TxnId,
    progress: &mut Progress,
) -> Result<Box<dyn Remote>, JobError> {
    progress.start(StepName::Verify).await;
    let deadline = Instant::now() + shared.timing.verify_uplink;
    let mut why;
    loop {
        tokio::time::sleep(shared.timing.poll).await;
        match manage::login(shared, node).await {
            Ok(remote) => {
                match healthy(shared, node, baseline, remote.as_ref()).await {
                    Ok(()) => {
                        progress
                            .done(Some("Reachable and as healthy as before".to_owned()))
                            .await;
                        return Ok(remote);
                    }
                    Err(regressions) => why = regressions,
                }
                if Instant::now() >= deadline {
                    let rolled = roll_back(remote.as_ref(), txn).await;
                    remote.close().await;
                    return Err(rolled_back(shared, &why, rolled));
                }
                remote.close().await;
            }
            Err(err) => why = format!("it didn't answer ({})", err.message),
        }
        if Instant::now() >= deadline {
            return Err(rolled_back(shared, &why, false));
        }
    }
}

async fn healthy(
    shared: &Shared,
    node: &Node,
    baseline: &Health,
    remote: &dyn Remote,
) -> Result<(), String> {
    let facts: NodeFacts = survey::collect(shared, remote, &SETS, &[Set::Board]).await?;
    let dns = survey::dns(remote, node.role).await;
    let regressions = Health::measure(node.role, &facts, dns).regressions(baseline);
    if regressions.is_empty() {
        Ok(())
    } else {
        Err(regressions.join(", "))
    }
}

/// Whether the rollback was done at once.
async fn roll_back(remote: &dyn Remote, txn: &TxnId) -> bool {
    let command = txn::rollback(txn);
    remote
        .exec(command.request.clone())
        .await
        .and_then(|output| command.parse(output))
        .is_ok()
}

fn rolled_back(shared: &Shared, why: &str, now: bool) -> JobError {
    JobError {
        code: JobErrorCode::RolledBack,
        message: if now {
            format!("After the restore {why}: it was rolled back and holds what it had")
        } else {
            format!(
                "After the restore {why}: it rolls back by itself within {} s of the apply",
                shared.timing.timer_uplink.as_secs()
            )
        },
    }
}
