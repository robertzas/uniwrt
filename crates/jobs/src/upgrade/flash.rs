//! Staging and flashing the image, and waiting for the node to come back (D-124).

use std::{path::Path, time::Duration};

use tokio::{io::AsyncReadExt as _, time::Instant};
use uniwrt_model::{
    firmware::FirmwareImage,
    job::{JobError, JobErrorCode, StepName},
    node::Node,
};
use uniwrt_node::{ExecRequest, Output, Target, proto::ops::Set};

use super::{STAGED, UpgradeTiming};
use crate::{
    Remote,
    apply::survey,
    errors::{internal, last_line, node_error},
    manage,
    progress::Progress,
    runner::Shared,
};

/// What is written to the node at a time.
const CHUNK: usize = 64 << 10;
/// Streaming the image: tens of MiB over the LAN or a mesh link.
const STAGE_TIMEOUT: Duration = Duration::from_secs(600);
/// `sha256sum` and `sysupgrade -T` read the whole image, on a slow CPU.
const CHECK_TIMEOUT: Duration = Duration::from_secs(120);
/// sysupgrade's own checks before it takes the node down and closes the sessions.
const FLASH_TIMEOUT: Duration = Duration::from_secs(90);

fn failed(message: impl Into<String>) -> JobError {
    JobError {
        code: JobErrorCode::NodeFailed,
        message: message.into(),
    }
}

/// Streams the image at `path` into [`STAGED`], reads its SHA-256 back and lets sysupgrade
/// test it (`-T`: the board, the metadata, the format).
pub(crate) async fn stage(
    remote: &dyn Remote,
    path: &Path,
    image: &FirmwareImage,
    progress: &mut Progress,
) -> Result<(), JobError> {
    progress.start(StepName::Stage).await;
    let size = tokio::time::timeout(STAGE_TIMEOUT, send(remote, path))
        .await
        .map_err(|_| {
            failed(format!(
                "The image wasn't copied to the node within {} min",
                STAGE_TIMEOUT.as_secs() / 60
            ))
        })??;
    let output = remote
        .exec(ExecRequest::new(["sha256sum", STAGED]).timeout(CHECK_TIMEOUT))
        .await
        .and_then(|output| output.check("sha256sum"))
        .map_err(|err| node_error(&err, "Cannot read the staged image's SHA-256"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let sha256 = stdout.split_whitespace().next().unwrap_or_default();
    if sha256 != image.sha256 {
        return Err(failed(format!(
            "The image staged on the node has SHA-256 {sha256}, not {}: the copy was damaged",
            image.sha256
        )));
    }
    let output = remote
        .exec(ExecRequest::new(["sysupgrade", "-T", STAGED]).timeout(CHECK_TIMEOUT))
        .await
        .map_err(|err| node_error(&err, "Cannot test the image"))?;
    if !output.status.success() {
        let said = if output.stderr.iter().any(|b| !b.is_ascii_whitespace()) {
            &output.stderr
        } else {
            &output.stdout
        };
        return Err(JobError {
            code: JobErrorCode::Refused,
            message: format!("sysupgrade refuses the image: {}", last_line(said)),
        });
    }
    progress
        .done(Some(format!(
            "{} MiB in /tmp; its SHA-256 and sysupgrade's test pass",
            size.div_ceil(1 << 20)
        )))
        .await;
    Ok(())
}

/// Copies the file at `path` to [`STAGED`] through `dd`'s stdin; returns its size.
async fn send(remote: &dyn Remote, path: &Path) -> Result<u64, JobError> {
    let reading = |err: std::io::Error| internal("reading the image", &err);
    let mut file = tokio::fs::File::open(path).await.map_err(reading)?;
    let argv = ["dd".to_owned(), format!("of={STAGED}"), "bs=64k".to_owned()];
    let copying = |err: uniwrt_node::Error| node_error(&err, "Cannot copy the image to the node");
    let mut stream = remote.stream(&argv).await.map_err(copying)?;
    let mut buffer = vec![0; CHUNK];
    let mut size = 0_u64;
    loop {
        let read = file.read(&mut buffer).await.map_err(reading)?;
        if read == 0 {
            break;
        }
        size += read as u64;
        stream
            .write(buffer[..read].to_vec())
            .await
            .map_err(copying)?;
    }
    stream.close_stdin();
    let mut stderr = Vec::new();
    while let Some(output) = stream.next().await {
        match output.map_err(copying)? {
            Output::Stderr(data) => stderr.extend_from_slice(&data),
            Output::Stdout(_) => {}
            Output::Exit(status) if status.success() => return Ok(size),
            Output::Exit(_) => {
                return Err(failed(format!(
                    "Cannot write the image to /tmp: {}",
                    last_line(&stderr)
                )));
            }
        }
    }
    Err(failed("The copy to the node ended without a status"))
}

/// Starts sysupgrade on the staged image, keeping the configuration. It closes the login as it
/// takes the node down, so a lost connection is what success looks like; only sysupgrade's own
/// refusal fails (and the image is deleted).
pub(crate) async fn flash(remote: &dyn Remote, progress: &mut Progress) -> Result<(), JobError> {
    progress.start(StepName::Flash).await;
    let started = remote
        .exec(ExecRequest::new(["sysupgrade", STAGED]).timeout(FLASH_TIMEOUT))
        .await;
    if let Ok(output) = started
        && !output.status.success()
    {
        let _ = remote.exec(ExecRequest::new(["rm", "-f", STAGED])).await;
        let said = [output.stderr, output.stdout].concat();
        return Err(failed(format!(
            "sysupgrade didn't start: {}",
            last_line(&said)
        )));
    }
    progress
        .done(Some(
            "sysupgrade is writing the image, keeping the configuration".to_owned(),
        ))
        .await;
    Ok(())
}

/// Waits for the node to go down (within `timing.down` of the flash, which started at `start`),
/// then to accept a login again (within `timing.back` of it), presenting its pinned host key. A
/// node that never went down and still runs `release` since before the flash wasn't upgraded.
pub(crate) async fn reboot(
    shared: &Shared,
    node: &Node,
    release: &str,
    start: Instant,
    timing: &UpgradeTiming,
    progress: &mut Progress,
) -> Result<Box<dyn Remote>, JobError> {
    progress.start(StepName::Reboot).await;
    let target = Target::new(node.host.clone(), node.port);
    let mut went_down = false;
    while start.elapsed() < timing.down {
        if !crate::reboot::answers(shared, &target).await {
            went_down = true;
            break;
        }
        tokio::time::sleep(timing.poll).await;
    }
    if went_down {
        progress.note("Down while the image is written").await;
    }
    loop {
        match manage::login(shared, node).await {
            Ok(remote) => {
                if !went_down {
                    not_rebooted(shared, node, remote.as_ref(), release, start).await?;
                }
                progress
                    .done(Some(format!(
                        "Back after {} s",
                        start.elapsed().as_secs().max(1)
                    )))
                    .await;
                return Ok(remote);
            }
            Err(err) if err.code == JobErrorCode::HostKeyMismatch => {
                return Err(JobError {
                    code: JobErrorCode::HostKeyMismatch,
                    message: format!(
                        "{} came back with another host key: the upgrade didn't keep its SSH \
                         keys. Make sure it is the device, then read its key again.",
                        node.name
                    ),
                });
            }
            Err(_) if start.elapsed() >= timing.back => {
                return Err(JobError {
                    code: JobErrorCode::NotBack,
                    message: format!(
                        "{} didn't come back within {} min of the flash. Its configuration is \
                         backed up on the controller. If it doesn't boot, recover it with its \
                         failsafe mode or TFTP recovery (its OpenWrt wiki page tells how).",
                        node.name,
                        timing.back.as_secs().div_ceil(60)
                    ),
                });
            }
            Err(_) => tokio::time::sleep(timing.poll).await,
        }
    }
}

/// Fails when the node never went down: it still runs `release` and has been up since before
/// the flash.
async fn not_rebooted(
    shared: &Shared,
    node: &Node,
    remote: &dyn Remote,
    release: &str,
    since: Instant,
) -> Result<(), JobError> {
    let facts = survey::collect(shared, remote, &[Set::Board, Set::System], &[Set::Board])
        .await
        .map_err(failed)?;
    let up = Duration::from_secs(facts.system.uptime_s);
    if facts.board.release.version == release && up >= since.elapsed() {
        return Err(failed(format!(
            "sysupgrade didn't take {} down: it still runs {release}, up for {} min",
            node.name,
            up.as_secs() / 60
        )));
    }
    Ok(())
}
