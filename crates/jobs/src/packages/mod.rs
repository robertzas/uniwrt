//! The package manager's job (features/network.md, Other services; D-139): an admin's
//! installs, removals and upgrades on one managed node. Steps: connect → check
//! ([`check::check`]: the names, the locks, the index refreshed, a dry run of each change, the
//! flash it takes; nothing changes) → install (removals, then upgrades, then installs: one
//! command each) → inventory (the packages read again and kept, also after a failed install).
//! Packages aren't configuration: nothing rolls back, and the site's revision is untouched.

mod check;
pub(crate) mod locks;
#[cfg(test)]
mod tests;

use std::{sync::Arc, time::Duration};

use tracing::{info, warn};
use uniwrt_core::UserId;
use uniwrt_model::{
    job::{JobError, JobErrorCode, JobKind, NewJob, StepName},
    node::Node,
    packages::PackageChanges,
};
use uniwrt_node::ExecRequest;

pub(crate) use self::check::read_catalog;
use self::check::{Plan, commands};
use crate::{
    Remote,
    errors::{last_line, node_error},
    firmware,
    manage::connect,
    progress::Progress,
    runner::Shared,
};

const STEPS: [StepName; 4] = [
    StepName::Connect,
    StepName::Check,
    StepName::Install,
    StepName::Inventory,
];
/// How long refreshing the index, a dry run or a change may take: slow mirrors on slow routers.
const TIMEOUT: Duration = Duration::from_secs(300);
/// The most a package manager may print.
const MAX_OUTPUT: usize = 1 << 20;
/// The most the catalog may take: every feed's packages with their descriptions (about 2 MiB).
pub(crate) const CATALOG_OUTPUT: usize = 16 << 20;

pub(crate) fn request(argv: Vec<String>) -> ExecRequest {
    let mut request = ExecRequest::new(argv);
    request.timeout = TIMEOUT;
    request.max_output = MAX_OUTPUT;
    request
}

pub(crate) fn job(node: &Node, by: Option<UserId>) -> NewJob {
    NewJob {
        kind: JobKind::Packages,
        target: node.name.clone(),
        node: Some(node.id),
        revision: None,
        created_by: by,
        steps: STEPS.to_vec(),
        nodes: Vec::new(),
    }
}

pub(crate) async fn run(
    shared: Arc<Shared>,
    node: Node,
    changes: PackageChanges,
    mut progress: Progress,
) {
    let result = packages(&shared, &node, &changes, &mut progress).await;
    match &result {
        Ok(()) => info!(node = %node.id, "packages changed"),
        Err(err) => warn!(node = %node.id, code = ?err.code, "{}", err.message),
    }
    progress.finish(result.err()).await;
}

async fn packages(
    shared: &Shared,
    node: &Node,
    changes: &PackageChanges,
    progress: &mut Progress,
) -> Result<(), JobError> {
    let remote = connect(shared, node, progress).await?;
    let plan = match check::check(shared, node, remote.as_ref(), changes, progress).await {
        Ok(plan) => plan,
        Err(err) => {
            remote.close().await;
            return Err(err);
        }
    };
    let installed = install(remote.as_ref(), &plan, progress).await;
    let result = match installed {
        Ok(()) => {
            progress.start(StepName::Inventory).await;
            match firmware::keep(shared, node, remote.as_ref(), plan.manager).await {
                Ok(inventory) => {
                    let detail = format!("{} packages installed", inventory.packages.len());
                    progress.done(Some(detail)).await;
                    Ok(())
                }
                Err(err) => Err(err),
            }
        }
        Err(err) => {
            // What did change shows on the device's page.
            if let Err(read) = firmware::keep(shared, node, remote.as_ref(), plan.manager).await {
                warn!(node = %node.id, "cannot read the packages again: {}", read.message);
            }
            Err(err)
        }
    };
    remote.close().await;
    result
}

/// The Install step: each change in turn; the first failure ends it, saying what went before.
async fn install(
    remote: &dyn Remote,
    plan: &Plan,
    progress: &mut Progress,
) -> Result<(), JobError> {
    progress.start(StepName::Install).await;
    let mut done: Vec<String> = Vec::new();
    for (argv, names) in commands(plan.manager, &plan.changes) {
        let verb = match argv[1].as_str() {
            "del" | "remove" => ("Removing", "removed"),
            "upgrade" => ("Upgrading", "upgraded"),
            _ => ("Installing", "installed"),
        };
        let names = names.join(", ");
        progress.note(format!("{} {names}", verb.0)).await;
        let failed = |why: String| {
            let before = if done.is_empty() {
                String::new()
            } else {
                format!(" ({} first)", done.join("; "))
            };
            JobError {
                code: JobErrorCode::NodeFailed,
                message: format!("{} {names} failed{before}: {why}", verb.0),
            }
        };
        let output = remote
            .exec(request(argv))
            .await
            .map_err(|err| failed(node_error(&err, "the command broke off").message))?;
        if !output.status.success() {
            let said = [output.stderr.as_slice(), output.stdout.as_slice()].concat();
            return Err(failed(last_line(&said)));
        }
        done.push(format!("{} {names}", verb.1));
    }
    let mut detail = done.join("; ");
    if let Some(first) = detail.get(..1) {
        detail = first.to_uppercase() + &detail[1..];
    }
    progress.done(Some(detail)).await;
    Ok(())
}

/// What the node's feeds offer (D-139): its index refreshed, then the packages and the upgrades
/// of `installed`. Only the index changes.
pub(crate) async fn catalog(
    remote: &dyn Remote,
    manager: uniwrt_model::facts::PackageManager,
    installed: &[uniwrt_model::inventory::Package],
    now: uniwrt_core::Timestamp,
) -> Result<uniwrt_model::packages::PackageCatalog, JobError> {
    let update = remote
        .exec(request(uniwrt_node::pkg::update(manager)))
        .await
        .map_err(|err| node_error(&err, "Cannot refresh the package index"))?;
    if !update.status.success() {
        let said = [update.stderr.as_slice(), update.stdout.as_slice()].concat();
        return Err(check::refused(format!(
            "Cannot refresh the package index: {}. Does the device reach its package feeds?",
            last_line(&said)
        )));
    }
    let available = read_catalog(remote, manager).await?.into_values().collect();
    let output = remote
        .exec(request(uniwrt_node::pkg::catalog::upgradable(manager)))
        .await
        .map_err(|err| node_error(&err, "Cannot read the upgrades"))?;
    if !output.status.success() {
        return Err(check::refused(format!(
            "Cannot read the upgrades: {}",
            last_line(&output.stderr)
        )));
    }
    let upgrades = uniwrt_node::pkg::catalog::parse_upgrades(
        manager,
        &String::from_utf8_lossy(&output.stdout),
        installed,
    );
    Ok(uniwrt_model::packages::PackageCatalog {
        read_at: now,
        available,
        upgrades,
    })
}
