//! The upgrade's steps before anything changes (D-124): the checks, with the health baseline,
//! and the backup.

use std::{collections::BTreeSet, time::Duration};

use tracing::warn;
use uniwrt_core::NodeId;
use uniwrt_model::{
    facts::NodeFacts,
    firmware::FirmwareImage,
    inventory::BackupReason,
    job::{JobError, JobErrorCode, StepName},
    log::{LogEntry, Severity},
    node::Node,
};
use uniwrt_node::{ExecRequest, logread, proto::ops::Set};
use uniwrt_syslog::Patterns;

use super::{
    Baseline, UpgradeRequest,
    image::{self, Boot},
};
use crate::{
    Remote,
    apply::{health::Health, survey},
    progress::Progress,
    runner::Shared,
};

/// The facts the checks and the baseline read.
pub(crate) const SETS: [Set; 6] = [
    Set::Board,
    Set::System,
    Set::Interfaces,
    Set::Radios,
    Set::Batman,
    Set::Stations,
];
/// Without these the checks can't be made.
pub(crate) const REQUIRED: [Set; 2] = [Set::Board, Set::System];

/// Room left in `/tmp` beside the image: sysupgrade's own files and the running services'.
const TMP_MARGIN: u64 = 16 << 20;

/// How far back the baseline's log reaches, from the node's newest entry (its own clock).
const BASELINE_LOG: Duration = Duration::from_secs(3600);

fn refused(message: impl Into<String>) -> JobError {
    JobError {
        code: JobErrorCode::Refused,
        message: message.into(),
    }
}

fn failed(message: impl Into<String>) -> JobError {
    JobError {
        code: JobErrorCode::NodeFailed,
        message: message.into(),
    }
}

/// Checks that the node can take the upgrade and picks its image; measures the baseline.
pub(crate) async fn check(
    shared: &Shared,
    node: &Node,
    request: &UpgradeRequest,
    remote: &dyn Remote,
    progress: &mut Progress,
) -> Result<(FirmwareImage, Baseline), JobError> {
    progress.start(StepName::Check).await;
    survey::idle_tool(remote).await.map_err(refused)?;
    let facts = survey::collect(shared, remote, &SETS, &REQUIRED)
        .await
        .map_err(failed)?;
    let release = facts.board.release.version.clone();
    if release != request.from {
        return Err(refused(format!(
            "{} runs {release} now, not {} as when the upgrade was prepared: prepare it again",
            node.name, request.from
        )));
    }
    let boot = if request.images.len() > 1 {
        boot(remote).await
    } else {
        Boot::default()
    };
    let image = image::choose(&request.images, &boot)
        .cloned()
        .ok_or_else(|| {
            refused(format!(
                "None of the images fits {} ({}{})",
                node.name,
                if boot.efi { "EFI" } else { "BIOS" },
                boot.filesystem
                    .as_deref()
                    .map_or_else(String::new, |fs| format!(", {fs}"))
            ))
        })?;
    tmp_room(&facts, &image)?;
    let logged = read_log(remote, node.id).await;
    let since = logged
        .last()
        .map(|newest| newest.time.saturating_sub(BASELINE_LOG));
    let baseline = Baseline {
        role: node.role,
        health: Health::measure(node.role, &facts, survey::dns(remote, node.role).await),
        clients: facts.stations.len(),
        patterns: error_patterns(logged.iter().filter(|entry| Some(entry.time) >= since)),
        release,
    };
    progress
        .done(Some(format!(
            "{} → {}: {}",
            baseline.release,
            request.to.as_deref().unwrap_or("another build"),
            image.name
        )))
        .await;
    Ok((image, baseline))
}

/// The image must fit in `/tmp` with [`TMP_MARGIN`] to spare.
fn tmp_room(facts: &NodeFacts, image: &FirmwareImage) -> Result<(), JobError> {
    let (Some(size), Some(tmp)) = (image.size_bytes, facts.system.tmp.as_ref()) else {
        return Ok(());
    };
    if tmp.available_bytes >= size + TMP_MARGIN {
        return Ok(());
    }
    Err(refused(format!(
        "/tmp has {} MiB free; the image needs {} MiB and 16 MiB to spare",
        tmp.available_bytes >> 20,
        size.div_ceil(1 << 20)
    )))
}

/// How the node boots and its root's file system.
async fn boot(remote: &dyn Remote) -> Boot {
    let efi = remote
        .exec(ExecRequest::new(["test", "-d", "/sys/firmware/efi"]))
        .await
        .is_ok_and(|output| output.status.success());
    let filesystem = remote
        .exec(ExecRequest::new(["cat", "/proc/mounts"]))
        .await
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| image::root_filesystem(&String::from_utf8_lossy(&output.stdout)));
    Boot { efi, filesystem }
}

/// The node's newest log entries, read over `remote` (`logread`), never from the store: syslog
/// feeds that, and any host on the LAN can forge a line (D-150). Empty when it can't be read.
pub(crate) async fn read_log(remote: &dyn Remote, node: NodeId) -> Vec<LogEntry> {
    match remote.exec(logread::request()).await {
        Ok(output) if output.status.success() => logread::parse(&output),
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!(%node, stderr = %stderr.trim(), "cannot read the node's log");
            Vec::new()
        }
        Err(err) => {
            warn!(%node, %err, "cannot read the node's log");
            Vec::new()
        }
    }
}

/// The log patterns of errors or worse that `entries` match.
pub(crate) fn error_patterns<'a>(entries: impl Iterator<Item = &'a LogEntry>) -> BTreeSet<String> {
    let patterns = Patterns::embedded();
    entries
        .filter(|entry| entry.severity <= Severity::Error)
        .filter_map(|entry| patterns.annotate(entry))
        .map(|pattern| pattern.id.clone())
        .collect()
}

/// Takes the configuration's backup and keeps it, sealed.
pub(crate) async fn backup(
    shared: &Shared,
    node: &Node,
    remote: &dyn Remote,
    progress: &mut Progress,
) -> Result<(), JobError> {
    progress.start(StepName::Backup).await;
    let kept = crate::backup::take(shared, node.id, remote, BackupReason::Upgrade).await?;
    progress.done(Some(kept)).await;
    Ok(())
}
