//! The wpad swap (features/wifi.md, Roaming; D-085, D-089): on a managed node, the basic wpad
//! (or the mesh one) gives way to the full one, which adds 802.11v. Steps: connect → check (no
//! open transaction, a hostapd without 802.11v, the wpad installed and its full counterpart,
//! free flash and RAM) → fetch (every package onto the node; nothing changes) → swap (detached
//! on the node and offline, [`uniwrt_node::wpad`]; the controller logs in again as often as it takes to read
//! how it ended) → verify (the agent finds 802.11v). The node's configuration doesn't change:
//! an apply then renders 802.11v where its WLANs ask for it.

#[cfg(test)]
mod tests;

use std::{sync::Arc, time::Duration};

use tracing::{info, warn};
use uniwrt_core::UserId;
use uniwrt_model::{
    facts::WifiFeature,
    job::{JobError, JobErrorCode, JobKind, NewJob, StepName},
    node::Node,
};
use uniwrt_node::{
    ExecRequest,
    proto::ops::Set,
    wpad::{self as swap, Outcome, Swap},
};

use crate::{
    Remote,
    apply::survey,
    errors::{chain, last_line, node_error, unsupported},
    manage::{connect, login},
    progress::Progress,
    runner::Shared,
};

const STEPS: [StepName; 5] = [
    StepName::Connect,
    StepName::Check,
    StepName::Fetch,
    StepName::Swap,
    StepName::Verify,
];
/// How long fetching may take: the indexes and three packages, from a slow mirror.
const FETCH_TIMEOUT: Duration = Duration::from_secs(300);
/// How long the swap may take on the node, until its Wi-Fi is back.
const SWAP_TIMEOUT: Duration = Duration::from_secs(240);
/// How often its result is read meanwhile.
const POLL: Duration = Duration::from_secs(2);
/// How long one read of the result may take, over a link the swap may have cut.
const READ_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) fn job(node: &Node, by: Option<UserId>) -> NewJob {
    NewJob {
        kind: JobKind::Wpad,
        target: node.name.clone(),
        node: Some(node.id),
        revision: None,
        created_by: by,
        steps: STEPS.to_vec(),
        nodes: Vec::new(),
    }
}

pub(crate) async fn run(shared: Arc<Shared>, node: Node, mut progress: Progress) {
    let result = wpad(&shared, &node, &mut progress).await;
    match &result {
        Ok(()) => info!(node = %node.id, "wpad swap done"),
        Err(err) => warn!(node = %node.id, code = ?err.code, "{}", err.message),
    }
    progress.finish(result.err()).await;
}

fn failed(message: impl Into<String>) -> JobError {
    JobError {
        code: JobErrorCode::NodeFailed,
        message: message.into(),
    }
}

async fn wpad(shared: &Shared, node: &Node, progress: &mut Progress) -> Result<(), JobError> {
    let remote = connect(shared, node, progress).await?;
    let swap = match check(shared, remote.as_ref(), progress).await {
        Ok(Some(swap)) => swap,
        Ok(None) => {
            remote.close().await;
            for step in [StepName::Fetch, StepName::Swap, StepName::Verify] {
                progress.skip(step, "Nothing to swap").await;
            }
            return Ok(());
        }
        Err(err) => {
            remote.close().await;
            return Err(err);
        }
    };
    if let Err(err) = fetch(remote.as_ref(), &swap, progress).await {
        // The packages go; nothing else changed.
        let _ = remote
            .exec(ExecRequest::new(["rm", "-rf", swap::DIR]))
            .await;
        remote.close().await;
        return Err(err);
    }
    let (remote, swapped) = run_swap(shared, node, remote, &swap, progress).await;
    let Some(remote) = remote else {
        return swapped;
    };
    let verified = match swapped {
        Ok(()) => verify(shared, remote.as_ref(), progress).await,
        Err(err) => Err(err),
    };
    remote.close().await;
    verified
}

/// Reads what the node has; `None` when its hostapd has 802.11v already.
async fn check(
    shared: &Shared,
    remote: &dyn Remote,
    progress: &mut Progress,
) -> Result<Option<Swap>, JobError> {
    progress.start(StepName::Check).await;
    match survey::idle_tool(remote).await {
        Ok(true) => {}
        Ok(false) => {
            return Err(failed(
                "The transaction tool is missing: turn management off and on again",
            ));
        }
        Err(why) => return Err(failed(why)),
    }
    let facts = survey::collect(shared, remote, &[Set::Capabilities], &[Set::Capabilities])
        .await
        .map_err(failed)?;
    let caps = &facts.capabilities;
    if caps.wifi.is_empty() {
        return Err(unsupported(
            "The device has no hostapd, so no Wi-Fi to serve".to_owned(),
        ));
    }
    if caps.wifi.contains(&WifiFeature::BssTransition) {
        progress
            .done(Some("Its wpad has 802.11v already".to_owned()))
            .await;
        return Ok(None);
    }
    let manager = caps.package_manager.ok_or_else(|| {
        unsupported("The device has neither apk nor opkg to install packages with".to_owned())
    })?;
    let installed = survey::installed(remote, &facts).await.map_err(failed)?;
    let swap = swap::choose(manager, &installed)
        .map_err(unsupported)?
        .ok_or_else(|| failed("Its wpad is the full one, yet hostapd lacks 802.11v"))?;
    for (path, need, what) in [
        ("/", swap::FLASH_KIB, "flash"),
        ("/tmp", swap::TMP_KIB, "memory in /tmp"),
    ] {
        let output = remote
            .exec(ExecRequest::new(["df", "-k", path]))
            .await
            .and_then(|output| output.check("df"))
            .map_err(|err| node_error(&err, "Cannot read its free space"))?;
        let free = swap::available_kib(&String::from_utf8_lossy(&output.stdout))
            .ok_or_else(|| failed(format!("Cannot read the free space of {path}")))?;
        if free < need {
            return Err(unsupported(format!(
                "The swap needs {need} KiB of free {what}; the device has {free} KiB"
            )));
        }
    }
    progress
        .done(Some(format!("{} in place of {}", swap.to, swap.from)))
        .await;
    Ok(Some(swap))
}

/// Downloads every package the swap needs onto the node.
async fn fetch(remote: &dyn Remote, swap: &Swap, progress: &mut Progress) -> Result<(), JobError> {
    progress.start(StepName::Fetch).await;
    for argv in swap::fetch(swap) {
        let what = if argv[0] == "sh" {
            "opkg download".to_owned()
        } else {
            argv[..2].join(" ")
        };
        let mut request = ExecRequest::new(argv);
        request.timeout = FETCH_TIMEOUT;
        request.max_output = 1 << 20;
        let output = remote
            .exec(request)
            .await
            .map_err(|err| node_error(&err, &format!("Nothing changed. {what} failed")))?;
        if !output.status.success() {
            let text = if output.stderr.is_empty() {
                &output.stdout
            } else {
                &output.stderr
            };
            return Err(failed(format!(
                "Nothing changed. {what} failed: {}. Does the device reach its package feeds?",
                last_line(text)
            )));
        }
    }
    progress
        .done(Some(format!(
            "{} and {} are on the device",
            swap.to, swap.from
        )))
        .await;
    Ok(())
}

/// Starts the swap on the node, then reads its result until it ends, logging in again whenever
/// the login drops; hands back the login it ends with, if any.
async fn run_swap(
    shared: &Shared,
    node: &Node,
    remote: Box<dyn Remote>,
    swap: &Swap,
    progress: &mut Progress,
) -> (Option<Box<dyn Remote>>, Result<(), JobError>) {
    progress.start(StepName::Swap).await;
    let started = remote.exec(ExecRequest::new(swap::launch(swap))).await;
    if let Ok(output) = &started
        && !output.status.success()
    {
        let why = format!(
            "Nothing changed: the swap didn't start: {}",
            last_line(&output.stderr)
        );
        return (Some(remote), Err(failed(why)));
    }
    // A login lost meanwhile may have started it: its result says.
    let deadline = tokio::time::Instant::now() + SWAP_TIMEOUT;
    let mut current = Some(remote);
    let outcome = loop {
        if current.is_none() {
            current = login(shared, node).await.ok();
        }
        if let Some(remote) = &current {
            let mut read = ExecRequest::new(["cat", &format!("{}/state", swap::DIR)]);
            read.timeout = READ_TIMEOUT;
            match remote.exec(read).await {
                Ok(output) if output.status.success() => {
                    if let Some(outcome) = Outcome::parse(&String::from_utf8_lossy(&output.stdout))
                    {
                        break Some(outcome);
                    }
                }
                Ok(_) => {}
                Err(_) => {
                    if let Some(lost) = current.take() {
                        lost.close().await;
                    }
                }
            }
        }
        if tokio::time::Instant::now() >= deadline {
            break None;
        }
        tokio::time::sleep(POLL).await;
    };
    let remote = match current {
        Some(remote) => remote,
        None => match login(shared, node).await {
            Ok(remote) => remote,
            Err(err) => {
                let why = format!(
                    "The swap didn't report back, and the device can't be reached: {}. Check its Wi-Fi",
                    err.message
                );
                progress.fail(&why).await;
                return (None, Err(failed(why)));
            }
        },
    };
    let result = match outcome {
        Some(Outcome::Swapped) => {
            progress
                .done(Some(format!(
                    "{} in place of {}; its Wi-Fi is back",
                    swap.to, swap.from
                )))
                .await;
            return (Some(remote), Ok(()));
        }
        Some(Outcome::Unchanged) => format!(
            "Nothing changed: the swap failed: {}",
            log_line(remote.as_ref()).await
        ),
        Some(Outcome::Restored) => format!(
            "{} didn't install, so {} is back: {}",
            swap.to,
            swap.from,
            log_line(remote.as_ref()).await
        ),
        Some(Outcome::Broken) => format!(
            "Neither wpad is installed, so the device's Wi-Fi is off: {}. Reach it by wire and install {}",
            log_line(remote.as_ref()).await,
            swap.from
        ),
        None => format!(
            "The swap didn't end within {} s; its log is {}/log on the device",
            SWAP_TIMEOUT.as_secs(),
            swap::DIR
        ),
    };
    progress.fail(&result).await;
    (Some(remote), Err(failed(result)))
}

/// The line of the swap's log that says why it failed: the package manager's last word.
async fn log_line(remote: &dyn Remote) -> String {
    let read = ExecRequest::new(["cat", &format!("{}/log", swap::DIR)]);
    let Ok(output) = remote.exec(read).await else {
        return "its log can't be read".to_owned();
    };
    let log = String::from_utf8_lossy(&output.stdout);
    let words: Vec<&str> = log
        .lines()
        .filter(|line| !line.starts_with("result: ") && *line != "bringing Wi-Fi up")
        .collect();
    last_line(words.join("\n").as_bytes())
}

/// Checks that the agent finds 802.11v now, and clears the swap's directory.
async fn verify(
    shared: &Shared,
    remote: &dyn Remote,
    progress: &mut Progress,
) -> Result<(), JobError> {
    progress.start(StepName::Verify).await;
    let facts = survey::collect(shared, remote, &[Set::Capabilities], &[Set::Capabilities])
        .await
        .map_err(failed)?;
    if !facts
        .capabilities
        .wifi
        .contains(&WifiFeature::BssTransition)
    {
        return Err(failed(
            "The full wpad is installed, yet the agent finds no 802.11v in hostapd",
        ));
    }
    if let Err(err) = remote
        .exec(ExecRequest::new(["rm", "-rf", swap::DIR]))
        .await
    {
        warn!(err = %chain(&err), "cannot clear the wpad swap's directory");
    }
    progress
        .done(Some(
            "802.11v is available: review and apply to turn it on".to_owned(),
        ))
        .await;
    Ok(())
}
