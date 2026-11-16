//! Upgrading a node's firmware (features/lifecycle.md, Firmware & rolling upgrades; D-015,
//! D-124). One managed node, the image the admin prepared:
//!
//! connect → check (the release the plan was made for, no open transaction, the image that fits
//! its firmware and root file system, room in `/tmp`; the health baseline) → backup
//! (`sysupgrade -b`, sealed and kept) → download (onto the controller, checked against its
//! SHA-256, through [`FetchImage`]) → stage (streamed into `/tmp`, its SHA-256 read back,
//! `sysupgrade -T`) → flash (`sysupgrade`, keeping the configuration) → reboot (down, then a login
//! again within [`UpgradeTiming::back`], the pinned host key required) → install (an official
//! image's packages, D-122: what fails is a warning) → verify (the release, the transaction
//! tool, the health baseline met again within [`UpgradeTiming::settle`]) → soak (still
//! reachable and healthy for [`UpgradeTiming::soak`]; no new error patterns in its log; the
//! clients back) → record (its packages read again).
//!
//! Nothing changes on the node before the image is staged and checked; a failure until then
//! deletes it. After the flash nothing can be rolled back: a failure says what the node runs and
//! how to recover it (the backup is on the controller; failsafe mode or TFTP when it doesn't
//! boot).

mod after;
mod before;
mod flash;
mod image;
mod packages;
#[cfg(test)]
mod tests;

use std::{collections::BTreeSet, path::PathBuf, sync::Arc, time::Duration};

use tracing::{info, warn};
use uniwrt_core::{NodeId, Timestamp, UserId};
use uniwrt_model::{
    firmware::FirmwareImage,
    job::{JobError, JobErrorCode, JobKind, NewJob, StepName},
    node::{Node, NodeRole},
};

use crate::{BoxFuture, Remote, apply::health::Health, manage, progress::Progress, runner::Shared};

/// What to upgrade a node with: the API resolves it from the plan the admin saw.
pub struct UpgradeRequest {
    pub node: NodeId,
    /// The release the node runs now, as the plan found it: another one refuses the job.
    pub from: String,
    /// The release the image holds, when known (ASU, official, an upload's metadata): the node
    /// must run it afterwards.
    pub to: Option<String>,
    /// The image's files to choose from: one, or x86's BIOS and EFI ones per file system.
    pub images: Vec<FirmwareImage>,
    /// What to install once an official image runs (D-122), as ASU would have baked it in: a
    /// name, or `-name` to remove. Empty for the other sources.
    pub packages: Vec<String>,
    /// Puts the chosen image on the controller's disk.
    pub fetch: Arc<dyn FetchImage>,
}

/// Puts an image on the controller's disk (the server's download cache, an upload's file).
pub trait FetchImage: Send + Sync + 'static {
    /// The image's file, checked against its SHA-256; why not, for people.
    fn fetch<'a>(&'a self, image: &'a FirmwareImage) -> BoxFuture<'a, Result<PathBuf, String>>;
}

/// How patiently an upgrade waits on its node. [`Default`] is production's.
#[derive(Debug, Clone)]
pub struct UpgradeTiming {
    /// Between two tries while the node goes down and comes back.
    pub poll: Duration,
    /// How long sysupgrade may take to take the node down.
    pub down: Duration,
    /// How long the node may take to accept a login again, from the flash.
    pub back: Duration,
    /// How long its radios and mesh may take to be as healthy as before, once it is back.
    pub settle: Duration,
    /// How long it is watched once healthy, and how often.
    pub soak: Duration,
    pub soak_every: Duration,
}

impl Default for UpgradeTiming {
    fn default() -> Self {
        Self {
            poll: Duration::from_secs(5),
            down: Duration::from_secs(180),
            back: Duration::from_secs(600),
            settle: Duration::from_secs(150),
            soak: Duration::from_secs(180),
            soak_every: Duration::from_secs(30),
        }
    }
}

const STEPS: [StepName; 11] = [
    StepName::Connect,
    StepName::Check,
    StepName::Backup,
    StepName::Download,
    StepName::Stage,
    StepName::Flash,
    StepName::Reboot,
    StepName::Install,
    StepName::Verify,
    StepName::Soak,
    StepName::Record,
];

/// Where the image is staged on the node.
pub(crate) const STAGED: &str = "/tmp/uniwrt-upgrade.img";

/// What the node was like before: what must hold again after.
pub(crate) struct Baseline {
    pub(crate) role: NodeRole,
    pub(crate) release: String,
    pub(crate) health: Health,
    pub(crate) clients: usize,
    /// The log patterns (error or worse) the node matched in the hour before.
    pub(crate) patterns: BTreeSet<String>,
}

pub(crate) fn job(node: &Node, request: &UpgradeRequest, by: Option<UserId>) -> NewJob {
    NewJob {
        kind: JobKind::Upgrade,
        target: match &request.to {
            Some(to) => format!("{} to {to}", node.name),
            None => node.name.clone(),
        },
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
    request: UpgradeRequest,
    mut progress: Progress,
) {
    let result = upgrade(&shared, &node, &request, &mut progress).await;
    match &result {
        Ok(()) => info!(node = %node.id, to = ?request.to, "firmware upgraded"),
        Err(err) => warn!(node = %node.id, code = ?err.code, "{}", err.message),
    }
    progress.finish(result.err()).await;
}

async fn upgrade(
    shared: &Shared,
    node: &Node,
    request: &UpgradeRequest,
    progress: &mut Progress,
) -> Result<(), JobError> {
    let remote = manage::connect(shared, node, progress).await?;
    let staged = prepare(shared, node, request, remote.as_ref(), progress).await;
    let (image, baseline) = match staged {
        Ok(staged) => staged,
        Err(err) => {
            // Nothing changed but the staged file, if any.
            let _ = remote
                .exec(uniwrt_node::ExecRequest::new(["rm", "-f", STAGED]))
                .await;
            remote.close().await;
            return Err(err);
        }
    };
    let flashed_at = shared.clock.now();
    let flashing = tokio::time::Instant::now();
    let started = flash::flash(remote.as_ref(), progress).await;
    remote.close().await;
    started?;
    let timing = &shared.timing.upgrade;
    let remote = flash::reboot(shared, node, &baseline.release, flashing, timing, progress).await?;
    let outcome = after::after(
        shared, node, request, &image, &baseline, flashed_at, remote, progress,
    )
    .await;
    outcome.map_err(|err| flashed(node, err))
}

/// The steps that change nothing but the staged file: check, backup, download, stage.
async fn prepare(
    shared: &Shared,
    node: &Node,
    request: &UpgradeRequest,
    remote: &dyn Remote,
    progress: &mut Progress,
) -> Result<(FirmwareImage, Baseline), JobError> {
    let (image, baseline) = before::check(shared, node, request, remote, progress).await?;
    before::backup(shared, node, remote, progress).await?;
    progress.start(StepName::Download).await;
    let path = request.fetch.fetch(&image).await.map_err(|why| JobError {
        code: JobErrorCode::Refused,
        message: format!("Cannot download {}: {why}", image.name),
    })?;
    progress
        .done(Some(format!("{}, SHA-256 checked", image.name)))
        .await;
    flash::stage(remote, &path, &image, progress).await?;
    Ok((image, baseline))
}

/// A failure once the node runs the new image: nothing can be rolled back, so it says so.
fn flashed(node: &Node, err: JobError) -> JobError {
    let JobError { code, message } = err;
    JobError {
        code,
        message: format!(
            "{} The new firmware is installed on {}; its configuration from before is backed up \
             on the controller.",
            message, node.name
        ),
    }
}

/// When the job looked at the node's log from: an hour before `now`.
pub(crate) fn log_window(now: Timestamp) -> Timestamp {
    now.saturating_sub(Duration::from_secs(3600))
}
