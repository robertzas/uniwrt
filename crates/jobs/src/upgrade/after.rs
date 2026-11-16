//! The upgrade's steps once the node is back (D-124): an official image's packages, the
//! checks against the baseline, the soak, and its packages recorded.

use std::{collections::BTreeSet, time::Duration};

use tokio::time::Instant;
use uniwrt_core::Timestamp;
use uniwrt_model::{
    facts::{NodeFacts, PackageManager},
    firmware::FirmwareImage,
    job::{JobError, JobErrorCode, StepName},
    node::Node,
};
use uniwrt_node::{ExecRequest, pkg, proto::ops::Set, txn::Footprint};

use super::{
    Baseline, UpgradeRequest,
    before::{SETS, error_patterns},
    packages::after_image,
};
use crate::{
    Remote,
    apply::{health::Health, survey},
    errors::{chain, last_line, txn_error},
    firmware, manage,
    progress::Progress,
    runner::Shared,
    wpad,
};

/// Refreshing the feeds and installing from them.
const PACKAGES_TIMEOUT: Duration = Duration::from_secs(300);

fn failed(message: impl Into<String>) -> JobError {
    JobError {
        code: JobErrorCode::NodeFailed,
        message: message.into(),
    }
}

/// The steps after the reboot, over `remote`, a login to the node back.
#[expect(
    clippy::too_many_arguments,
    reason = "the job's state, passed along once"
)]
pub(crate) async fn after(
    shared: &Shared,
    node: &Node,
    request: &UpgradeRequest,
    image: &FirmwareImage,
    baseline: &Baseline,
    flashed_at: Timestamp,
    remote: Box<dyn Remote>,
    progress: &mut Progress,
) -> Result<(), JobError> {
    let mut remote = install(shared, node, request, remote, progress).await?;
    let result = async {
        let facts = verify(shared, request, baseline, remote.as_ref(), progress).await?;
        soak(shared, node, baseline, flashed_at, &mut remote, progress).await?;
        record(shared, node, image, &facts, remote.as_ref(), progress).await
    }
    .await;
    remote.close().await;
    result
}

/// Installs an official image's packages; what fails is a warning (the step fails, the job
/// goes on). Hands back a login: the wpad swap may have logged in again.
async fn install(
    shared: &Shared,
    node: &Node,
    request: &UpgradeRequest,
    remote: Box<dyn Remote>,
    progress: &mut Progress,
) -> Result<Box<dyn Remote>, JobError> {
    if request.packages.is_empty() {
        progress
            .skip(StepName::Install, "The image holds its packages")
            .await;
        return Ok(remote);
    }
    progress.start(StepName::Install).await;
    let read = survey::collect(shared, remote.as_ref(), &[Set::Capabilities], &[])
        .await
        .map_err(failed)?;
    let Some(manager) = read.capabilities.package_manager else {
        progress
            .fail("The new release has no package manager uniwrt knows: nothing installed")
            .await;
        return Ok(remote);
    };
    let installed = match survey::installed(remote.as_ref(), &read).await {
        Ok(installed) => installed,
        Err(why) => {
            progress.fail(format!("Nothing installed: {why}")).await;
            return Ok(remote);
        }
    };
    let plan = after_image(&request.packages, &installed);
    let mut problems = Vec::new();
    let added = add(remote.as_ref(), manager, &plan.install, &mut problems).await;
    let mut removed = 0;
    if !plan.remove.is_empty() {
        match run(remote.as_ref(), pkg::remove(manager, &plan.remove)).await {
            Ok(()) => removed = plan.remove.len(),
            Err(why) => problems.push(format!("{} not removed ({why})", plan.remove.join(", "))),
        }
    }
    let mut remote = remote;
    if plan.full_wpad {
        let (back, swapped) = wpad::within(shared, node, remote, progress).await;
        remote = match back {
            Some(remote) => remote,
            None => manage::login(shared, node).await?,
        };
        if let Err(err) = swapped {
            problems.push(format!("the full wpad isn't in ({})", err.message));
        }
    }
    let summary = format!(
        "{added} installed, {removed} removed{}",
        if plan.full_wpad {
            ", the full wpad"
        } else {
            ""
        }
    );
    if problems.is_empty() {
        progress.done(Some(summary)).await;
    } else {
        progress
            .fail(format!("{summary}; {}", problems.join("; ")))
            .await;
    }
    Ok(remote)
}

/// Installs `packages` at once, else one by one to find which fail; returns how many went in.
async fn add(
    remote: &dyn Remote,
    manager: PackageManager,
    packages: &[String],
    problems: &mut Vec<String>,
) -> usize {
    if packages.is_empty() {
        return 0;
    }
    if let Err(why) = run(remote, pkg::update(manager)).await {
        problems.push(format!(
            "the feeds can't be read ({why}): nothing installed"
        ));
        return 0;
    }
    if run(remote, pkg::install(manager, packages)).await.is_ok() {
        return packages.len();
    }
    let mut added = 0;
    for package in packages {
        match run(remote, pkg::install(manager, std::slice::from_ref(package))).await {
            Ok(()) => added += 1,
            Err(why) => problems.push(format!("{package} not installed ({why})")),
        }
    }
    added
}

async fn run(remote: &dyn Remote, argv: Vec<String>) -> Result<(), String> {
    let output = remote
        .exec(ExecRequest::new(argv).timeout(PACKAGES_TIMEOUT))
        .await
        .map_err(|err| chain(&err))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(last_line(&[output.stderr, output.stdout].concat()))
    }
}

/// The node runs the release, has the transaction tool, and is as healthy as before within
/// `settle`. Returns its facts.
async fn verify(
    shared: &Shared,
    request: &UpgradeRequest,
    baseline: &Baseline,
    remote: &dyn Remote,
    progress: &mut Progress,
) -> Result<NodeFacts, JobError> {
    progress.start(StepName::Verify).await;
    let timing = &shared.timing.upgrade;
    let mut facts = read(shared, remote).await?;
    let running = &facts.board.release.version;
    if let Some(to) = &request.to
        && running != to
    {
        return Err(failed(format!(
            "It runs {running}, not {to}: the image didn't take"
        )));
    }
    let reinstalled = tool(remote).await?;
    let deadline = Instant::now() + timing.settle;
    loop {
        let regressions = health(remote, baseline, &facts).await;
        if regressions.is_empty() {
            break;
        }
        if Instant::now() >= deadline {
            return Err(failed(format!(
                "After the upgrade {}",
                regressions.join(", ")
            )));
        }
        tokio::time::sleep(timing.poll).await;
        facts = read(shared, remote).await?;
    }
    progress
        .done(Some(format!(
            "Runs {} {}; as healthy as before{}",
            facts.board.release.version,
            facts.board.release.revision,
            if reinstalled {
                "; the transaction tool installed again"
            } else {
                ""
            }
        )))
        .await;
    Ok(facts)
}

async fn read(shared: &Shared, remote: &dyn Remote) -> Result<NodeFacts, JobError> {
    let sets: Vec<Set> = SETS.into_iter().chain([Set::Capabilities]).collect();
    survey::collect(shared, remote, &sets, &[Set::Board, Set::System])
        .await
        .map_err(failed)
}

async fn health(remote: &dyn Remote, baseline: &Baseline, facts: &NodeFacts) -> Vec<String> {
    let dns = survey::dns(remote, baseline.role).await;
    Health::measure(baseline.role, facts, dns).regressions(&baseline.health)
}

/// Installs the transaction tool again unless the upgrade kept this version (its files are in
/// sysupgrade's keep list); returns whether it did.
async fn tool(remote: &dyn Remote) -> Result<bool, JobError> {
    let footprint = Footprint::get();
    let status = manage::status(remote).await?;
    if status.is_some_and(|status| status.version == footprint.version()) {
        return Ok(false);
    }
    let command = footprint.install();
    let what = "Installing the transaction tool again";
    let output = remote
        .exec(command.request.clone())
        .await
        .map_err(|err| txn_error(&err, what))?;
    command.parse(output).map_err(|err| txn_error(&err, what))?;
    Ok(true)
}

/// Watches the node for `timing.soak`: reachable (logging in again once if the login drops) and
/// as healthy as before; then no new error patterns in its log since the flash. Fewer clients
/// than before is only noted: they come and go.
async fn soak(
    shared: &Shared,
    node: &Node,
    baseline: &Baseline,
    flashed_at: Timestamp,
    remote: &mut Box<dyn Remote>,
    progress: &mut Progress,
) -> Result<(), JobError> {
    progress.start(StepName::Soak).await;
    let timing = &shared.timing.upgrade;
    let start = Instant::now();
    let mut clients;
    loop {
        let left = timing.soak.saturating_sub(start.elapsed());
        tokio::time::sleep(left.min(timing.soak_every)).await;
        let facts = if let Ok(facts) = read(shared, remote.as_ref()).await {
            facts
        } else {
            *remote = manage::login(shared, node)
                .await
                .map_err(|err| failed(format!("It went down during the soak: {}", err.message)))?;
            read(shared, remote.as_ref()).await?
        };
        let regressions = health(remote.as_ref(), baseline, &facts).await;
        if !regressions.is_empty() {
            return Err(failed(format!(
                "During the soak {}",
                regressions.join(", ")
            )));
        }
        clients = facts.stations.len();
        if start.elapsed() >= timing.soak {
            break;
        }
    }
    let now = shared.clock.now();
    let new: BTreeSet<String> = error_patterns(shared, node.id, flashed_at, now)
        .await
        .difference(&baseline.patterns)
        .cloned()
        .collect();
    if !new.is_empty() {
        return Err(failed(format!(
            "Its log shows errors it didn't have before: {}",
            new.into_iter().collect::<Vec<_>>().join(", ")
        )));
    }
    let minutes = timing.soak.as_secs().div_ceil(60);
    let back = if clients >= baseline.clients {
        String::new()
    } else {
        format!(
            "; {clients} of its {} clients back so far",
            baseline.clients
        )
    };
    progress
        .done(Some(format!("Healthy for {minutes} min{back}")))
        .await;
    Ok(())
}

/// Reads the packages again and keeps them as its inventory.
async fn record(
    shared: &Shared,
    node: &Node,
    image: &FirmwareImage,
    facts: &NodeFacts,
    remote: &dyn Remote,
    progress: &mut Progress,
) -> Result<(), JobError> {
    progress.start(StepName::Record).await;
    let kept = match facts.capabilities.package_manager {
        Some(manager) => firmware::keep(shared, node, remote, manager)
            .await
            .map(|inventory| format!("; {} packages", inventory.packages.len()))?,
        None => String::new(),
    };
    progress
        .done(Some(format!(
            "{} {} from {}{kept}",
            facts.board.release.distribution, facts.board.release.version, image.name
        )))
        .await;
    Ok(())
}
