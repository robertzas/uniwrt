//! The speed test's run: one login to the gateway, the script once per phase, and the result
//! recorded.

use std::{sync::Arc, time::Duration};

use tracing::info;
use uniwrt_model::{
    facts::NodeFacts,
    job::{JobError, JobErrorCode, JobNodeState, StepName},
    node::Node,
    site::gateway::HEALTH_TARGETS,
    speed::{Grade, SpeedTest},
};
use uniwrt_node::{ExecRequest, proto::ops::Set};
use uniwrt_store::SpeedTestRepo as _;

use super::{
    DOWNLOAD_URL, IDLE_PINGS, LOAD_S, STREAMS, SpeedTestRequest, UPLOAD_URL,
    measure::{self, Direction, Phase},
};
use crate::{
    Remote,
    apply::survey,
    errors::{chain, internal},
    progress::Progress,
    runner::Shared,
};

/// The script, sent on stdin each phase: nothing is left on the gateway.
const SCRIPT: &str = include_str!("../../../../node-agent/speedtest.sh");
/// The most a phase may print: a line a second and a reply a second.
const MAX_OUTPUT: usize = 64 * 1024;

pub(crate) async fn run(
    shared: Arc<Shared>,
    gateway: Node,
    request: SpeedTestRequest,
    mut progress: Progress,
) {
    let result = test(&shared, &gateway, request, &mut progress).await;
    if let Err(err) = &result {
        info!(code = ?err.code, "speed test failed: {}", err.message);
    }
    progress.finish(result.err()).await;
}

fn failed(code: JobErrorCode, message: impl Into<String>) -> JobError {
    JobError {
        code,
        message: message.into(),
    }
}

async fn test(
    shared: &Shared,
    node: &Node,
    request: SpeedTestRequest,
    progress: &mut Progress,
) -> Result<(), JobError> {
    progress.start(StepName::Connect).await;
    let (_, remote) = match survey::login(shared, node, &node.host).await {
        Ok(login) => login,
        Err(why) => {
            progress.fail(why.clone()).await;
            return Err(failed(JobErrorCode::Unreachable, why));
        }
    };
    let result = phases(shared, node, remote.as_ref(), request, progress).await;
    remote.close().await;
    result
}

/// The WAN's layer-3 device: `wan`'s, else that of another WAN interface with a default route.
fn device(facts: &NodeFacts) -> Option<String> {
    let up = |name: &str| {
        facts
            .interfaces
            .iter()
            .find(|i| i.name == name && i.up)
            .and_then(|i| i.device.clone())
    };
    up("wan").or_else(|| {
        facts
            .interfaces
            .iter()
            .find(|i| i.name.starts_with("wan") && i.up && i.ipv4_gateway.is_some())
            .and_then(|i| i.device.clone())
    })
}

/// The script's run of `phase`.
async fn script(
    remote: &dyn Remote,
    phase: &str,
    device: &str,
    bypass: bool,
) -> Result<Phase, String> {
    let (count, url) = match phase {
        "idle" => (IDLE_PINGS, "-"),
        "down" => (LOAD_S, DOWNLOAD_URL),
        _ => (LOAD_S, UPLOAD_URL),
    };
    let target = HEALTH_TARGETS[1].to_string();
    let argv = [
        "sh",
        "-s",
        "--",
        phase,
        device,
        &count.to_string(),
        &STREAMS.to_string(),
        if bypass { "1" } else { "0" },
        url,
        &target,
    ];
    let mut request = ExecRequest::new(argv).stdin(SCRIPT);
    request.timeout = Duration::from_secs(u64::from(count) + 30);
    request.max_output = MAX_OUTPUT;
    let output = remote.exec(request).await.map_err(|err| chain(&err))?;
    let phase = measure::parse(&output.stdout);
    match phase.err.as_deref() {
        Some("no_fetch") => Err("The gateway has no uclient-fetch".to_owned()),
        Some("no_device") => Err(format!("The gateway has no device {device}")),
        Some(other) => Err(format!("The speed test failed on the gateway: {other}")),
        None => Ok(phase),
    }
}

fn ms(us: u32) -> String {
    format!("{:.1} ms", f64::from(us) / 1000.0)
}

fn mbps(kbps: u32) -> String {
    format!("{:.1} Mbit/s", f64::from(kbps) / 1000.0)
}

async fn phases(
    shared: &Shared,
    node: &Node,
    remote: &dyn Remote,
    request: SpeedTestRequest,
    progress: &mut Progress,
) -> Result<(), JobError> {
    let facts = survey::collect(shared, remote, &[Set::Interfaces], &[Set::Interfaces]).await;
    let Some(device) = facts.as_ref().ok().and_then(device) else {
        let why = "The WAN is down: no WAN interface is up".to_owned();
        progress.fail(why.clone()).await;
        return Err(failed(JobErrorCode::NoInternet, why));
    };
    progress.done(Some(format!("Measuring on {device}"))).await;

    progress.start(StepName::Idle).await;
    let idle = script(remote, "idle", &device, false).await;
    let Some(idle_us) = idle.as_ref().ok().and_then(|p| measure::median(&p.rtts_us)) else {
        let why = idle
            .err()
            .unwrap_or_else(|| format!("The gateway gets no reply from {}", HEALTH_TARGETS[1]));
        progress.fail(why.clone()).await;
        return Err(failed(JobErrorCode::NoInternet, why));
    };
    progress.done(Some(ms(idle_us))).await;

    let mut loads = Vec::new();
    for (step, phase, direction) in [
        (StepName::Download, "down", Direction::Down),
        (StepName::Upload, "up", Direction::Up),
    ] {
        progress.start(step).await;
        let measured = script(remote, phase, &device, request.bypass_sqm)
            .await
            .and_then(|p| {
                let rate = measure::rate_kbps(&p.samples, direction)
                    .filter(|&kbps| kbps > 0)
                    .ok_or_else(|| "Nothing moved: the transfers didn't start".to_owned())?;
                Ok((rate, measure::p90(&p.rtts_us, LOAD_S as usize)))
            });
        match measured {
            Ok((kbps, us)) => {
                progress
                    .done(Some(format!("{}, {} under load", mbps(kbps), ms(us))))
                    .await;
                loads.push((kbps, us));
            }
            Err(why) => {
                progress.fail(why.clone()).await;
                return Err(failed(JobErrorCode::NoInternet, why));
            }
        }
    }
    let [(download_kbps, download_us), (upload_kbps, upload_us)] = loads[..] else {
        return Err(failed(JobErrorCode::Internal, "a load phase is missing"));
    };

    progress.start(StepName::Record).await;
    let test = SpeedTest {
        job: progress.job_id(),
        node: node.id,
        at: shared.clock.now(),
        bypassed: request.bypass_sqm,
        download_kbps,
        upload_kbps,
        idle_us,
        download_us,
        upload_us,
        grade: Grade::of(idle_us, download_us.max(upload_us)),
    };
    shared
        .store
        .record_speed_test(test.clone())
        .await
        .map_err(|err| internal("recording the speed test", &err))?;
    progress
        .node(
            node.id,
            JobNodeState::Confirmed,
            format!("Grade {}", test.grade.label()),
        )
        .await;
    progress
        .done(Some(format!(
            "{} down, {} up, bufferbloat grade {}",
            mbps(download_kbps),
            mbps(upload_kbps),
            test.grade.label()
        )))
        .await;
    Ok(())
}
