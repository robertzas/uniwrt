//! The Check step (D-139): the changes asked for, against the node as it is. Nothing changes
//! on the node but its package index: the names, the locks, a dry run of each change, and the
//! flash the new packages take.

use std::collections::{BTreeMap, BTreeSet};

use uniwrt_model::{
    facts::{NodeFacts, PackageManager},
    job::{JobError, JobErrorCode, StepName},
    node::Node,
    packages::{AvailablePackage, PackageChanges, PackageLock, with_firmware},
};
use uniwrt_node::{
    ExecRequest,
    pkg::{self, catalog, dry_run},
    proto::ops::Set,
    txn::current,
};
use uniwrt_store::RevisionRepo as _;
use uniwrt_uci::Package;

use super::{CATALOG_OUTPUT, locks, request};
use crate::{
    Remote,
    apply::{plan, survey},
    errors::{chain, internal, last_line, node_error, unsupported},
    progress::Progress,
    runner::Shared,
};

/// Flash kept free besides the new packages: a tenth of the overlay, at least this.
const RESERVE_MIN: u64 = 1 << 20;

/// What the Install step runs.
#[derive(Debug, Clone)]
pub(crate) struct Plan {
    pub(crate) manager: PackageManager,
    pub(crate) changes: PackageChanges,
}

pub(crate) fn refused(message: impl Into<String>) -> JobError {
    JobError {
        code: JobErrorCode::Refused,
        message: message.into(),
    }
}

pub(crate) async fn check(
    shared: &Shared,
    node: &Node,
    remote: &dyn Remote,
    changes: &PackageChanges,
    progress: &mut Progress,
) -> Result<Plan, JobError> {
    progress.start(StepName::Check).await;
    let facts = survey::collect(shared, remote, &Set::ALL, &survey::RENDER_SETS)
        .await
        .map_err(refused)?;
    let manager = facts.capabilities.package_manager.ok_or_else(|| {
        unsupported("The device has neither apk nor opkg to install packages with".to_owned())
    })?;
    let installed = survey::installed(remote, &facts).await.map_err(refused)?;
    let site = shared
        .store
        .head_revision()
        .await
        .map_err(|err| internal("reading the latest revision", &err))?
        .map(|revision| revision.config);
    let needs = locks::needs(node, &facts, site.as_ref());
    let required = if changes.remove.is_empty() {
        Ok(BTreeSet::new())
    } else {
        let current = current_packages(remote).await?;
        locks::required(shared, node, site.as_ref(), &facts, &current).await
    };
    let locked = |name: &str| match &required {
        Ok(required) => locks::lock(name, needs, required),
        Err(_) => locks::lock(name, needs, &BTreeSet::new()),
    };
    names(changes, &installed, &required, &locked)?;

    progress.note("Refreshing the package index").await;
    run(
        remote,
        pkg::update(manager),
        "Nothing changed. Refreshing the index",
    )
    .await?;
    progress.note("Reading what the feeds offer").await;
    let available = read_catalog(remote, manager).await?;
    let upgrades = read_upgrades(remote, manager).await?;
    if let Some(name) = changes
        .install
        .iter()
        .find(|name| !available.contains_key(*name))
    {
        return Err(refused(format!(
            "{name} isn't in the device's package feeds"
        )));
    }
    if let Some(name) = changes
        .upgrade
        .iter()
        .find(|name| !upgrades.contains(*name))
    {
        return Err(refused(format!(
            "{name} is the newest version the feeds have"
        )));
    }

    progress.note("Trying the changes").await;
    let tried = dry_runs(remote, manager, changes).await?;
    if let Some(name) = tried.removed.iter().find(|name| locked(name).is_some()) {
        return Err(refused(format!(
            "That would remove {name} along, which the device needs"
        )));
    }
    if let Some(name) = tried.upgraded.iter().find(|name| with_firmware(name)) {
        return Err(refused(format!(
            "That would upgrade {name}, which only comes with a firmware upgrade"
        )));
    }
    let rom = read_rom(remote, manager).await;
    let needed = flash_needed(&tried, &available, &rom);
    space(&facts, needed)?;
    progress.done(Some(summary(&tried, needed))).await;
    Ok(Plan {
        manager,
        changes: changes.clone(),
    })
}

/// Each change tried in turn; refused when the manager refuses one, keeps a removal back, or
/// would remove nothing.
async fn dry_runs(
    remote: &dyn Remote,
    manager: PackageManager,
    changes: &PackageChanges,
) -> Result<dry_run::DryRun, JobError> {
    let mut tried = dry_run::DryRun::default();
    for (argv, names) in commands(manager, changes) {
        let output = remote
            .exec(request(dry_run::simulate(argv)))
            .await
            .map_err(|err| node_error(&err, "Nothing changed. The dry run failed"))?;
        let run = dry_run::parse(&String::from_utf8_lossy(&output.stdout));
        if let Some((name, needed)) = run.kept.first() {
            return Err(refused(format!(
                "{name} can't go: {} needs it",
                needed.join(", ")
            )));
        }
        if !output.status.success() {
            let said = [output.stderr.as_slice(), output.stdout.as_slice()].concat();
            return Err(refused(format!(
                "Nothing changed. The package manager refused: {}",
                last_line(&said)
            )));
        }
        if let Some(name) = names.iter().find(|name| {
            changes.remove.contains(name) && !run.removed.iter().any(|gone| gone == *name)
        }) {
            return Err(refused(format!("Removing {name} would remove nothing")));
        }
        tried.added.extend(run.added);
        tried.upgraded.extend(run.upgraded);
        tried.removed.extend(run.removed);
    }
    Ok(tried)
}

/// Refuses names that aren't installed, are locked, are named twice or come with the firmware.
fn names(
    changes: &PackageChanges,
    installed: &BTreeSet<String>,
    required: &Result<BTreeSet<String>, String>,
    locked: &dyn Fn(&str) -> Option<PackageLock>,
) -> Result<(), JobError> {
    let mut seen = BTreeSet::new();
    if let Some(name) = changes.names().find(|name| !seen.insert(*name)) {
        return Err(refused(format!("{name} is named twice")));
    }
    if let Some(name) = changes
        .install
        .iter()
        .find(|name| installed.contains(*name))
    {
        return Err(refused(format!("{name} is installed already")));
    }
    for name in changes.remove.iter().chain(&changes.upgrade) {
        if !installed.contains(name) {
            return Err(refused(format!("{name} isn't installed")));
        }
    }
    if !changes.remove.is_empty()
        && let Err(why) = required
    {
        return Err(refused(format!("Nothing can be removed now. {why}")));
    }
    for name in &changes.remove {
        match locked(name) {
            Some(PackageLock::Required) => {
                return Err(refused(format!(
                    "{name} is locked: the site's configuration needs it"
                )));
            }
            Some(PackageLock::Essential) => {
                return Err(refused(format!("{name} is locked: the device needs it")));
            }
            None => {}
        }
    }
    if let Some(name) = changes.upgrade.iter().find(|name| with_firmware(name)) {
        return Err(refused(format!(
            "{name} comes with the firmware: upgrade the firmware instead"
        )));
    }
    Ok(())
}

/// The commands of `changes`, each with the names it is given: removals first (they free
/// flash), then upgrades, then installs.
pub(crate) fn commands(
    manager: PackageManager,
    changes: &PackageChanges,
) -> Vec<(Vec<String>, Vec<String>)> {
    [
        (pkg::uninstall(manager, &changes.remove), &changes.remove),
        (pkg::upgrade(manager, &changes.upgrade), &changes.upgrade),
        (pkg::install(manager, &changes.install), &changes.install),
    ]
    .into_iter()
    .filter(|(_, names)| !names.is_empty())
    .map(|(argv, names)| (argv, names.clone()))
    .collect()
}

async fn current_packages(remote: &dyn Remote) -> Result<BTreeMap<String, Package>, JobError> {
    let packages = current::packages(&plan::packages())
        .map_err(|err| internal("listing the packages", &err))?;
    let files = remote
        .exec(packages.request.clone())
        .await
        .and_then(|output| packages.parse(output))
        .map_err(|err| refused(format!("Cannot read its configuration: {}", chain(&err))))?;
    let mut current = BTreeMap::new();
    for (name, text) in files {
        let Some(text) = text else { continue };
        // A file that doesn't parse renders as if absent: render reports what it needs.
        if let Ok(package) = uniwrt_uci::parse(&name, &text) {
            current.insert(name, package);
        }
    }
    Ok(current)
}

async fn run(remote: &dyn Remote, argv: Vec<String>, what: &str) -> Result<Vec<u8>, JobError> {
    let command = argv.join(" ");
    let output = remote
        .exec(request(argv))
        .await
        .map_err(|err| node_error(&err, &format!("{what} failed")))?;
    if !output.status.success() {
        let said = [output.stderr.as_slice(), output.stdout.as_slice()].concat();
        return Err(refused(format!(
            "{what} failed ({command}: {}). Does the device reach its package feeds?",
            last_line(&said)
        )));
    }
    Ok(output.stdout)
}

/// The feeds' packages, by name.
pub(crate) async fn read_catalog(
    remote: &dyn Remote,
    manager: PackageManager,
) -> Result<BTreeMap<String, AvailablePackage>, JobError> {
    let mut read = request(catalog::catalog(manager));
    read.max_output = CATALOG_OUTPUT;
    let output = remote
        .exec(read)
        .await
        .map_err(|err| node_error(&err, "Cannot read the package feeds"))?;
    if !output.status.success() {
        return Err(refused(format!(
            "Cannot read the package feeds: {}",
            last_line(&output.stderr)
        )));
    }
    Ok(
        catalog::parse_catalog(&String::from_utf8_lossy(&output.stdout))
            .into_iter()
            .map(|package| (package.name.clone(), package))
            .collect(),
    )
}

/// The names of the installed packages with a newer version.
async fn read_upgrades(
    remote: &dyn Remote,
    manager: PackageManager,
) -> Result<BTreeSet<String>, JobError> {
    let stdout = run(remote, catalog::upgradable(manager), "Reading the upgrades").await?;
    Ok(
        catalog::parse_upgrades(manager, &String::from_utf8_lossy(&stdout), &[])
            .into_iter()
            .map(|upgrade| upgrade.name)
            .collect(),
    )
}

/// The firmware's own packages; none where it has no `/rom`.
async fn read_rom(remote: &dyn Remote, manager: PackageManager) -> BTreeSet<String> {
    match remote.exec(ExecRequest::new(catalog::rom(manager))).await {
        Ok(output) if output.status.success() => {
            catalog::parse_rom(&String::from_utf8_lossy(&output.stdout))
                .into_iter()
                .collect()
        }
        _ => BTreeSet::new(),
    }
}

/// The flash the changes take: each new package whole, and an upgrade of a package of the
/// firmware's (its old version stays in `/rom`); an upgrade on the overlay replaces the old.
fn flash_needed(
    tried: &dry_run::DryRun,
    available: &BTreeMap<String, AvailablePackage>,
    rom: &BTreeSet<String>,
) -> u64 {
    let size = |name: &String| {
        available
            .get(name)
            .and_then(|package| package.installed_bytes)
            .unwrap_or(0)
    };
    let added: u64 = tried.added.iter().map(size).sum();
    let upgraded: u64 = tried
        .upgraded
        .iter()
        .filter(|name| rom.contains(*name))
        .map(size)
        .sum();
    added + upgraded
}

fn space(facts: &NodeFacts, needed: u64) -> Result<(), JobError> {
    let Some(root) = &facts.system.root else {
        return Ok(());
    };
    let reserve = (root.total_bytes / 10).max(RESERVE_MIN);
    if needed > 0 && needed.saturating_add(reserve) > root.available_bytes {
        return Err(refused(format!(
            "Nothing changed. That needs {} of flash, and the device has {} free (keeping {} spare)",
            mib(needed),
            mib(root.available_bytes),
            mib(reserve)
        )));
    }
    Ok(())
}

pub(crate) fn mib(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss)]
    let mib = bytes as f64 / f64::from(1u32 << 20);
    format!("{mib:.1} MiB")
}

fn summary(tried: &dry_run::DryRun, needed: u64) -> String {
    let mut parts = Vec::new();
    if !tried.removed.is_empty() {
        parts.push(format!("removes {}", tried.removed.join(", ")));
    }
    if !tried.upgraded.is_empty() {
        parts.push(format!("upgrades {}", tried.upgraded.join(", ")));
    }
    if !tried.added.is_empty() {
        parts.push(format!(
            "installs {} ({})",
            tried.added.join(", "),
            mib(needed)
        ));
    }
    let mut text = parts.join("; ");
    if let Some(first) = text.get(..1) {
        text = first.to_uppercase() + &text[1..];
    }
    text
}
