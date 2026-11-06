//! The WAN test's run: one trial on the gateway (two with a 6rd fallback), its live checks, and
//! the rollback every path after an apply ends with.

use std::sync::Arc;

use uniwrt_isp::Step;
use uniwrt_model::{
    facts::NodeFacts,
    job::{JobError, JobErrorCode, JobNodeState, StepName},
    node::Node,
    site::{
        SecretRef, SiteConfig,
        gateway::{WanConfig, WanProto},
        node::{ManagedPackage, NodeConfig},
        revision::Revision,
    },
};
use uniwrt_node::ExecOutput;

use super::{
    WanTest,
    checks::{self, Address, Route},
    gateway::Gateway,
};
use crate::{
    apply::{health, survey},
    progress::Progress,
    runner::Shared,
};

/// The reference a password not in the vault yet takes while it is tried.
const TRIAL_SECRET: &str = "config/00000000000000000000000000000000";

pub(crate) async fn run(
    shared: Arc<Shared>,
    head: Revision,
    gateway: Node,
    test: WanTest,
    mut progress: Progress,
) {
    let result = trial(&shared, &head, &gateway, test, &mut progress).await;
    progress.finish(result.err()).await;
}

pub(super) fn failed(code: JobErrorCode, message: impl Into<String>) -> JobError {
    JobError {
        code,
        message: message.into(),
    }
}

async fn trial(
    shared: &Shared,
    head: &Revision,
    node: &Node,
    mut test: WanTest,
    progress: &mut Progress,
) -> Result<(), JobError> {
    progress.start(StepName::Connect).await;
    let (_, remote) = survey::login(shared, node, &node.host)
        .await
        .map_err(|why| failed(JobErrorCode::Unreachable, why))?;
    match survey::idle_tool(remote.as_ref()).await {
        Ok(true) => {}
        Ok(false) => {
            return Err(failed(
                JobErrorCode::NodeFailed,
                "The transaction tool is missing: turn management off and on again",
            ));
        }
        Err(why) => return Err(failed(JobErrorCode::NodeFailed, why)),
    }
    progress
        .done(Some(format!("Logged in to {}", node.host)))
        .await;

    progress.start(StepName::Plan).await;
    let mut gw = Gateway::read(shared, node, remote).await?;
    let mut site = head.config.clone();
    let password = test.password.take();
    if let (Some(_), WanProto::Pppoe { password: slot, .. }) = (&password, &mut test.wan.proto) {
        *slot = SecretRef::new(TRIAL_SECRET.to_owned());
    }
    site.wan = test.wan.clone();
    // Render writes only the packages a node manages: the trial needs `network`, whatever the
    // gateway manages (only its WAN sections are patched in).
    if !site.nodes.iter().any(|config| config.node == node.id) {
        site.nodes.push(NodeConfig::new(node.id));
    }
    for config in site
        .nodes
        .iter_mut()
        .filter(|config| config.node == node.id)
    {
        config.managed.insert(ManagedPackage::Network);
    }
    let mut secrets = survey::secrets(shared, &site).await?;
    if let (Some(value), Some(secret)) = (password, SecretRef::new(TRIAL_SECRET.to_owned())) {
        secrets.insert(secret, value);
    }
    let file = gw.render(&site, &secrets)?;
    let port = gw.facts.board.layout.wan.clone().unwrap_or_default();
    progress
        .done(Some(format!("Only the WAN changes, on {port}")))
        .await;

    let result = async {
        gw.try_file(
            &format!("uniwrt-{}", progress.job_id().get()),
            file,
            progress,
        )
        .await?;
        let outcome = connectivity(&gw, &test.wan, &port, progress).await;
        if let Some(fallback) = test.fallback.clone() {
            if outcome.ipv6_failed && outcome.failure.is_none() {
                site.wan.ipv6 = fallback;
                sixrd(&mut gw, &site, &secrets, progress).await;
            } else {
                let why = if outcome.ipv6_failed {
                    "IPv4 must work first"
                } else {
                    "Not needed"
                };
                progress.skip(StepName::SixRd, why).await;
            }
        }
        outcome.failure.map_or(Ok(()), Err)
    }
    .await;
    progress.start(StepName::Rollback).await;
    let detail = gw.roll_back().await;
    progress
        .node(node.id, JobNodeState::RolledBack, &detail)
        .await;
    progress.done(Some(detail)).await;
    gw.remote.close().await;
    result
}

/// The second trial: IPv6 by the preset's 6rd, after DHCPv6 delegated no prefix.
async fn sixrd(
    gw: &mut Gateway<'_>,
    site: &SiteConfig,
    secrets: &survey::VaultSecrets,
    progress: &mut Progress,
) {
    progress.start(StepName::SixRd).await;
    let timing = &gw.shared.timing;
    let result = async {
        let file = gw.render(site, secrets).map_err(|e| e.message)?;
        gw.roll_back().await;
        gw.apply(&format!("uniwrt-{}-6rd", progress.job_id().get()), file)
            .await?;
        gw.wait(timing.wan_wait, |f| {
            matches!(checks::address(f, &site.wan), Address::Up(_)).then_some(())
        })
        .await
        .0
        .ok_or("IPv4 didn't come back with 6rd")?;
        let (found, _) = gw
            .wait(timing.wan_wait * 3 / 4, |f| checks::ipv6(f, &site.wan.ipv6))
            .await;
        let found = found.ok_or_else(|| uniwrt_isp::hint(Step::Ipv6, &site.wan, None, &[]))?;
        let ping = gw.exec(checks::ping(checks::PING6, true)).await;
        match ping.as_ref().filter(|out| checks::succeeded(out)) {
            Some(out) => Ok(format!(
                "6rd works: {found}; {} answers{}",
                checks::PING6,
                rtt(out)
            )),
            None => Err(uniwrt_isp::hint(Step::Ipv6, &site.wan, None, &[])),
        }
    }
    .await;
    match result {
        Ok(detail) => progress.done(Some(detail)).await,
        Err(why) => progress.fail(why).await,
    }
}

/// What the checks found.
struct Outcome {
    /// Why IPv4 doesn't reach the internet.
    failure: Option<JobError>,
    ipv6_failed: bool,
}

/// The hints of the WAN tried: its preset's, else the generic ones.
struct Hints<'a> {
    wan: &'a WanConfig,
    preset: Option<&'static uniwrt_isp::Preset>,
}

impl Hints<'_> {
    fn get(&self, step: Step, facts: Option<&NodeFacts>) -> String {
        let errors = facts.map(checks::errors).unwrap_or_default();
        uniwrt_isp::hint(step, self.wan, self.preset, &errors)
    }
}

/// The checks, each reported live; the first IPv4 failure skips the steps that need it.
async fn connectivity(
    gw: &Gateway<'_>,
    wan: &WanConfig,
    port: &str,
    progress: &mut Progress,
) -> Outcome {
    let hints = Hints {
        wan,
        preset: wan.preset.as_deref().and_then(uniwrt_isp::preset),
    };
    let no_internet = |why: String| Some(failed(JobErrorCode::NoInternet, why));
    let facts = match uplink(gw, &hints, port, progress).await {
        Ok(facts) => facts,
        Err(why) => {
            return Outcome {
                failure: no_internet(why),
                ipv6_failed: false,
            };
        }
    };
    let failure = internet(gw, &hints, &facts, progress)
        .await
        .and_then(no_internet);
    let ipv6_failed = ipv6(gw, &hints, progress).await;
    Outcome {
        failure,
        ipv6_failed,
    }
}

/// Link, address and route; the facts read last, or why they failed (the rest skipped).
async fn uplink(
    gw: &Gateway<'_>,
    hints: &Hints<'_>,
    port: &str,
    progress: &mut Progress,
) -> Result<NodeFacts, String> {
    let (wan, wait) = (hints.wan, gw.shared.timing.wan_wait);
    progress.start(StepName::Link).await;
    let (link, facts) = gw
        .wait(wait / 3, |f| checks::link(f, port).then_some(()))
        .await;
    if link.is_none() {
        return Err(cut(
            progress,
            hints.get(Step::Link, facts.as_ref()),
            StepName::Address,
        )
        .await);
    }
    progress.done(Some(format!("{port} has a link"))).await;

    progress.start(StepName::Address).await;
    let (address, facts) = gw
        .wait(wait, |f| match checks::address(f, wan) {
            Address::Up(what) => Some(Ok(what)),
            Address::Refused(errors) => Some(Err(errors)),
            Address::Waiting => None,
        })
        .await;
    let Some(Ok(address)) = address else {
        let why = hints.get(Step::Address, facts.as_ref());
        return Err(cut(progress, why, StepName::Gateway).await);
    };
    progress.done(Some(address)).await;

    progress.start(StepName::Gateway).await;
    let (route, facts) = gw.wait(wait / 3, |f| checks::route(f, wan)).await;
    let detail = match route {
        None => {
            let why = hints.get(Step::Gateway, facts.as_ref());
            return Err(cut(progress, why, StepName::Dns).await);
        }
        Some(Route::Tunnel) => "Default route through the tunnel".to_owned(),
        Some(Route::Via(next)) => {
            let ping = gw
                .exec(checks::ping(&next.to_string(), next.is_ipv6()))
                .await;
            let answer = match ping.as_ref().filter(|out| checks::succeeded(out)) {
                Some(out) => format!("it answers{}", rtt(out)),
                None => "it doesn't answer pings".to_owned(),
            };
            format!("Default route via {next}; {answer}")
        }
    };
    progress.done(Some(detail)).await;
    Ok(facts.unwrap_or_default())
}

/// Fails the running step with `why` and skips the checks from `next` on.
async fn cut(progress: &mut Progress, why: String, next: StepName) -> String {
    progress.fail(why.clone()).await;
    let steps = [
        StepName::Address,
        StepName::Gateway,
        StepName::Dns,
        StepName::Https,
        StepName::Ipv6,
        StepName::SixRd,
    ];
    let start = steps.iter().position(|s| *s == next).unwrap_or(0);
    for step in &steps[start..] {
        progress.skip(*step, "Needs IPv4 on the WAN").await;
    }
    why
}

/// DNS and HTTPS; why they failed, if they did.
async fn internet(
    gw: &Gateway<'_>,
    hints: &Hints<'_>,
    facts: &NodeFacts,
    progress: &mut Progress,
) -> Option<String> {
    progress.start(StepName::Dns).await;
    let mut resolvers = Vec::new();
    let isp = checks::isp_dns(facts).map(|s| (s, " (the ISP's)"));
    for (server, whose) in isp.into_iter().chain([(checks::PUBLIC_DNS, "")]) {
        let out = gw.exec(checks::nslookup(server)).await;
        if out.as_ref().is_some_and(health::resolved) {
            resolvers.push(format!("{server}{whose}"));
        }
    }
    if resolvers.is_empty() {
        let why = hints.get(Step::Dns, Some(facts));
        progress.fail(why.clone()).await;
        progress.skip(StepName::Https, "Needs DNS").await;
        return Some(why);
    }
    let detail = format!(
        "{} resolves through {}",
        checks::NAME,
        resolvers.join(" and ")
    );
    progress.done(Some(detail)).await;

    progress.start(StepName::Https).await;
    match gw.exec(checks::https()).await {
        Some(out) if checks::succeeded(&out) => {
            let detail = format!("{} answers over HTTPS", checks::HTTPS);
            progress.done(Some(detail)).await;
            None
        }
        Some(out) if checks::missing(&out) => {
            progress.skip(StepName::Https, "wget isn't installed").await;
            None
        }
        _ => {
            let why = hints.get(Step::Https, Some(facts));
            progress.fail(why.clone()).await;
            Some(why)
        }
    }
}

/// IPv6: whether it failed (it's skipped when off).
async fn ipv6(gw: &Gateway<'_>, hints: &Hints<'_>, progress: &mut Progress) -> bool {
    let wan = hints.wan;
    if !wan.ipv6.is_on() {
        progress.skip(StepName::Ipv6, "IPv6 is off").await;
        return false;
    }
    progress.start(StepName::Ipv6).await;
    let wait = gw.shared.timing.wan_wait * 3 / 4;
    let (found, facts) = gw.wait(wait, |f| checks::ipv6(f, &wan.ipv6)).await;
    let ping = match &found {
        Some(_) => gw.exec(checks::ping(checks::PING6, true)).await,
        None => None,
    };
    match (found, ping.as_ref().filter(|out| checks::succeeded(out))) {
        (Some(found), Some(out)) => {
            let detail = format!("{found}; {} answers{}", checks::PING6, rtt(out));
            progress.done(Some(detail)).await;
            false
        }
        (found, _) => {
            let why = match found {
                Some(found) => format!("{found}, but {} doesn't answer", checks::PING6),
                None => hints.get(Step::Ipv6, facts.as_ref()),
            };
            progress.fail(why).await;
            true
        }
    }
}

fn rtt(out: &ExecOutput) -> String {
    checks::rtt(out).map_or_else(String::new, |rtt| format!(" in {rtt}"))
}
