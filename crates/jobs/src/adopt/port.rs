//! The adopt job's path for a factory-default device on the gateway's adoption port
//! (features/adoption.md, Path C; D-107). Through the gateway's login (`direct-tcpip` to
//! 192.168.1.1), it runs Path A's steps, installs the transaction tool (a factory device has
//! nothing to observe, so it is managed at once), and applies the staging config
//! (`uniwrt_render::staging`) as a transaction. Once the device has left 192.168.1.1, the gateway
//! lends the port to the management network ([`uniwrt_node::adoption_port::lend`]) until a
//! deadline a tenth of its timer before the transaction's own, so a device that rolls back is never
//! on the LAN; the job finds the device's lease by MAC in the gateway's leases, logs in there,
//! confirms, keeps the lend and records the node at that address. Any failure after the lend
//! gives the port back to the segment, and the device's timer rolls it back. Last, on the LAN,
//! the device gets the mesh's packages its image lacks ([`super::prepare`], D-108), then an
//! apply job of its own gives the node the revision the gateway holds ([`apply::adopted`]);
//! the device stays adopted whatever their outcome (the Configure step says so).

use std::{
    collections::{BTreeMap, BTreeSet},
    net::IpAddr,
    sync::Arc,
};

use serde::de::DeserializeOwned;
use tokio::time::{Instant, sleep};
use tracing::{debug, warn};
use uniwrt_core::NodeId;
use uniwrt_model::{
    facts::NodeFacts,
    job::{JobError, JobErrorCode, StepName},
    node::{Node, NodeRole},
    site::adoption::AdoptionPort,
};
use uniwrt_node::{
    Credential, ExecOutput, ExecRequest, Target,
    adoption_port::{self as lend, Lent},
    proto::ops::Set,
    txn::{self, OpenState, TxnId},
};
use uniwrt_store::{NodeRepo as _, RevisionRepo as _};

use super::AdoptRequest;
use crate::{
    Dial, Remote,
    apply::{self, Configured},
    errors::{chain, internal, node_error, txn_error},
    manage,
    progress::Progress,
    runner::Shared,
};

/// The port path's steps, in order.
pub(crate) const STEPS: [StepName; 17] = [
    StepName::Connect,
    StepName::Check,
    StepName::InstallKey,
    StepName::VerifyKey,
    StepName::SetPassword,
    StepName::Inventory,
    StepName::Backup,
    StepName::InstallTool,
    StepName::Stage,
    StepName::Apply,
    StepName::Lease,
    StepName::Verify,
    StepName::Confirm,
    StepName::Record,
    StepName::Watch,
    StepName::Prepare,
    StepName::Configure,
];

/// Where the device was found: the gateway whose adoption port it is plugged into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortPath {
    pub gateway: NodeId,
}

/// The lend ends a tenth of the staging transaction's timer before it runs out (30 s of
/// production's 5 minutes), and a lend shorter than that isn't worth starting (a lease, a
/// login and a confirmation).
const LEND_SHARE: u32 = 10;
/// The packages the staging config may change.
const STAGED: [&str; 3] = ["network", "dhcp", "wireless"];

/// What the saved site says of the port.
struct Site {
    gateway: Node,
    port: AdoptionPort,
    /// The management network's interface, which the port is lent to.
    interface: String,
}

pub(super) async fn adopt(
    shared: &Arc<Shared>,
    request: &AdoptRequest,
    path: &PortPath,
    progress: &mut Progress,
) -> Result<(), JobError> {
    if !matches!(request.role, Some(NodeRole::Ap | NodeRole::Bridge)) {
        return Err(refused(
            "A device on the adoption port is adopted as an access point or a bridge",
        ));
    }
    let site = site(shared, path).await?;
    let mut gateway = Gateway {
        remote: manage::login(shared, &site.gateway)
            .await
            .map_err(|err| JobError {
                message: format!("Cannot log in to the gateway: {}", err.message),
                ..err
            })?,
        shared,
        node: site.gateway.clone(),
    };
    let result = through(shared, request, &site, &mut gateway, progress).await;
    gateway.remote.close().await;
    result
}

async fn site(shared: &Shared, path: &PortPath) -> Result<Site, JobError> {
    let config = shared
        .store
        .head_revision()
        .await
        .map_err(|err| internal("reading the site", &err))?
        .map(|head| head.config);
    let Some((config, port)) =
        config.and_then(|config| config.adoption_port.clone().map(|port| (config, port)))
    else {
        return Err(refused("The site has no adoption port"));
    };
    let interface = uniwrt_render::management_network(&config)
        .ok_or_else(|| refused("The site has no network to lend the port to"))?
        .id
        .clone();
    let gateway = shared
        .store
        .node(path.gateway)
        .await
        .map_err(|err| internal("reading the gateway", &err))?
        .filter(|node| node.role == NodeRole::Gateway)
        .ok_or_else(|| refused("The device's gateway is no longer the site's gateway"))?;
    Ok(Site {
        gateway,
        port,
        interface,
    })
}

async fn through(
    shared: &Arc<Shared>,
    request: &AdoptRequest,
    site: &Site,
    gateway: &mut Gateway<'_>,
    progress: &mut Progress,
) -> Result<(), JobError> {
    let status = gateway.text(lend::segment_status()).await;
    let up = status.is_some_and(|status| lend::segment_up(&status, site.port.address));
    let Some(tunnel) = gateway.remote.dial().filter(|_| up) else {
        return Err(refused(
            "The adoption port's segment is not up on the gateway: plug the device in again",
        ));
    };
    let (found, device) = super::on_node(shared, tunnel.as_ref(), request, progress).await?;
    let job = progress.job_id();
    let staged = stage(device.as_ref(), progress, job).await;
    let txn = match staged {
        Ok(txn) => txn,
        Err(err) => {
            device.close().await;
            return Err(err);
        }
    };

    progress.start(StepName::Apply).await;
    let timeout = shared.timing.timer_uplink;
    let armed_at = Instant::now();
    let applied = tool(device.as_ref(), &txn::apply(&txn, timeout)).await;
    device.close().await;
    applied.map_err(|err| txn_error(&err, "Applying the staging config"))?;
    let margin = timeout / LEND_SHARE;
    let lend_until = armed_at + timeout.saturating_sub(margin);
    let gone_by =
        (armed_at + shared.timing.verify).min(lend_until.checked_sub(margin).unwrap_or(armed_at));
    if !gone(shared, tunnel.as_ref(), gone_by).await {
        roll_back(shared, tunnel.as_ref(), request, &txn).await;
        return Err(JobError {
            code: JobErrorCode::RolledBack,
            message: "The device still answers at 192.168.1.1 after applying the staging \
                      config: rolled back"
                .to_owned(),
        });
    }
    progress
        .done(Some(
            "Applied with a timer; the device left 192.168.1.1 to ask for a lease".to_owned(),
        ))
        .await;

    let host = match on_lan(
        shared,
        request,
        site,
        &found.facts,
        &txn,
        gateway,
        lend_until,
        progress,
    )
    .await
    {
        Ok(host) => host,
        Err(err) => {
            let back = gateway.lend(lend::end_lend(&site.port.port), true).await;
            let left = (armed_at + timeout).saturating_duration_since(Instant::now());
            let port = if back == Some(Lent::Ended) {
                "The port is back on the adoption segment"
            } else {
                "The port returns to the adoption segment by itself"
            };
            return Err(JobError {
                message: format!(
                    "{}. {port}; the device rolls back to 192.168.1.1 within {} s, then it \
                     can be adopted again",
                    err.message,
                    left.as_secs()
                ),
                ..err
            });
        }
    };
    let facts = found.facts.clone();
    let node = super::record(shared, request, found, host.to_string(), true, progress).await?;
    super::watch(shared, request, node.clone(), facts, progress).await;
    configure(shared, site.gateway.id, node, progress).await;
    Ok(())
}

/// On the LAN, the mesh's packages, then the node's own render from the revision the gateway
/// holds (step 5). The device is adopted whatever their outcome: a failure leaves it managed
/// with the staging config.
async fn configure(shared: &Arc<Shared>, gateway: NodeId, node: Node, progress: &mut Progress) {
    let keeps = "The node keeps the staging config (Wi-Fi off): apply the site from Review before \
                 unplugging it";
    let revision = match apply::held(shared, gateway).await {
        Ok(revision) => revision,
        Err(why) => {
            progress
                .skip(StepName::Prepare, "No revision to prepare for")
                .await;
            progress.start(StepName::Configure).await;
            progress.fail(format!("{why}. {keeps}")).await;
            return;
        }
    };
    if super::prepare::prepare(shared, &revision.config.mesh, &node, progress)
        .await
        .is_err()
    {
        progress.start(StepName::Configure).await;
        let why = "Not applied: the device lacks the mesh's packages";
        progress.fail(format!("{why}. {keeps}")).await;
        return;
    }
    progress.start(StepName::Configure).await;
    match apply::adopted(shared, &revision, node, progress.created_by()).await {
        Ok(Configured {
            job,
            revision,
            result: Ok(()),
        }) => {
            let detail = format!("Applied revision {revision} in job {job}");
            progress.done(Some(detail)).await;
        }
        Ok(Configured {
            job,
            revision,
            result: Err(err),
        }) => {
            let detail = format!(
                "Applying revision {revision} failed in job {job}: {}. {keeps}",
                err.message
            );
            progress.fail(detail).await;
        }
        Err(why) => progress.fail(format!("{why}. {keeps}")).await,
    }
}

/// Installs the transaction tool, then stages and checks the staging config.
async fn stage(
    device: &dyn Remote,
    progress: &mut Progress,
    job: uniwrt_core::JobId,
) -> Result<TxnId, JobError> {
    manage::install(device, progress).await?;
    progress.start(StepName::Stage).await;
    let what = "Staging the staging config";
    let read = txn::current::packages(&STAGED).map_err(|err| internal(what, &err))?;
    let output = device
        .exec(read.request.clone())
        .await
        .map_err(|err| node_error(&err, "Cannot read the device's configuration"))?;
    let files = read
        .parse(output)
        .map_err(|err| node_error(&err, "Cannot read the device's configuration"))?;
    let mut current = BTreeMap::new();
    for (name, text) in files {
        let Some(text) = text else { continue };
        let package = uniwrt_uci::parse(&name, &text).map_err(|err| JobError {
            code: JobErrorCode::NodeFailed,
            message: format!("The device's /etc/config/{name} doesn't parse: {err}"),
        })?;
        current.insert(name, package);
    }
    let staging = uniwrt_render::staging(&current).map_err(|issue| JobError {
        code: JobErrorCode::Unsupported,
        message: format!(
            "Cannot stage the device ({}): {}",
            issue.path, issue.message
        ),
    })?;
    let txn =
        TxnId::new(format!("uniwrt-{}-staging", job.get())).map_err(|err| internal(what, &err))?;
    let services = staging.services.iter().map(|(name, on)| (*name, *on));
    let staged = async {
        tool(device, &txn::stage(&txn, services)?).await?;
        for (name, package) in &staging.packages {
            tool(device, &txn::put(&txn, name, package.to_file())?).await?;
        }
        tool(device, &txn::check(&txn)).await
    }
    .await
    .map_err(|err| txn_error(&err, what))?;
    progress
        .done(Some(format!(
            "Staged {}: the LAN asks for a lease and serves none, the WAN port joins it, Wi-Fi \
             is off",
            staged.packages.join(", ")
        )))
        .await;
    Ok(txn)
}

/// Waits until two looks in a row find nothing at 192.168.1.1; `false` if it still answers at
/// `until`.
async fn gone(shared: &Shared, tunnel: &dyn Dial, until: Instant) -> bool {
    let target = factory();
    let mut misses = 0;
    loop {
        match tunnel.banner(&target).await {
            Ok(Some(_)) => misses = 0,
            _ => misses += 1,
        }
        if misses >= 2 {
            return true;
        }
        if Instant::now() >= until {
            return false;
        }
        sleep(shared.timing.poll).await;
    }
}

/// Rolls the device back through the tunnel, if it can still be reached; its timer does it
/// otherwise.
async fn roll_back(shared: &Shared, tunnel: &dyn Dial, request: &AdoptRequest, txn: &TxnId) {
    let login = tunnel
        .connect(
            &factory(),
            &request.host_key,
            &Credential::Key(shared.key.clone()),
        )
        .await;
    match login {
        Ok(device) => {
            if let Err(err) = tool(device.as_ref(), &txn::rollback(txn)).await {
                warn!(%err, "cannot roll the staging config back");
            }
            device.close().await;
        }
        Err(err) => warn!(%err, "cannot reach the device to roll it back"),
    }
}

/// The lend, the lease, the login at it and the confirmation; returns the device's address.
#[expect(
    clippy::too_many_arguments,
    reason = "one call site, each argument distinct"
)]
async fn on_lan(
    shared: &Shared,
    request: &AdoptRequest,
    site: &Site,
    facts: &NodeFacts,
    txn: &TxnId,
    gateway: &mut Gateway<'_>,
    lend_until: Instant,
    progress: &mut Progress,
) -> Result<IpAddr, JobError> {
    progress.start(StepName::Lease).await;
    let lasting = lend_until.saturating_duration_since(Instant::now());
    if lasting < shared.timing.timer_uplink / LEND_SHARE {
        return Err(failed("Too little time is left to lend the port"));
    }
    let port = &site.port.port;
    match gateway
        .lend(lend::lend(port, &site.interface, lasting), false)
        .await
    {
        Some(Lent::Lent) => {}
        Some(Lent::Busy) => return Err(failed("The gateway's adoption port is lent already")),
        _ => return Err(failed("The gateway cannot lend the adoption port")),
    }
    // Any of its MACs: the bridge's may change as the WAN port joins it.
    let macs: BTreeSet<_> = facts
        .devices
        .iter()
        .filter_map(|device| device.mac)
        .collect();
    let host = loop {
        let leased =
            gateway.leases().await.into_iter().find(|lease| {
                lease.ip.is_ipv4() && lease.mac.is_some_and(|mac| macs.contains(&mac))
            });
        if let Some(lease) = leased {
            break lease.ip;
        }
        if Instant::now() + shared.timing.poll >= lend_until {
            return Err(failed(&format!(
                "The device took no lease on the {} network within {} s",
                site.interface,
                lasting.as_secs()
            )));
        }
        sleep(shared.timing.poll).await;
    };
    progress
        .done(Some(format!(
            "Lent the port to the {} network; the device leased {host}",
            site.interface
        )))
        .await;

    progress.start(StepName::Verify).await;
    let device = login_at(shared, request, host, txn, lend_until).await?;
    progress
        .done(Some(format!(
            "Logged in at {host}; the staging config is applied"
        )))
        .await;

    progress.start(StepName::Confirm).await;
    let confirmed = tool(device.as_ref(), &txn::confirm(txn)).await;
    device.close().await;
    confirmed.map_err(|err| txn_error(&err, "Confirming the staging config"))?;
    let kept = gateway.lend(lend::keep(port), true).await;
    let detail = if kept == Some(Lent::Kept) {
        "Kept the staging config; the port stays on the LAN until the device is unplugged"
    } else {
        warn!(?kept, "cannot keep the adoption port's lend");
        "Kept the staging config; the port returns to the adoption segment soon, so unplug the \
         device and place it"
    };
    progress.done(Some(detail.to_owned())).await;
    Ok(host)
}

/// Logs in at the device's lease and waits for its transaction to be applied.
async fn login_at(
    shared: &Shared,
    request: &AdoptRequest,
    host: IpAddr,
    txn: &TxnId,
    until: Instant,
) -> Result<Box<dyn Remote>, JobError> {
    let Some(dial) = &shared.dial else {
        return Err(internal("no way to reach nodes", &"no dial"));
    };
    let target = Target::new(host.to_string(), request.target.port);
    let credential = Credential::Key(shared.key.clone());
    loop {
        let why = match dial.connect(&target, &request.host_key, &credential).await {
            Ok(device) => {
                let status = tool(device.as_ref(), &txn::status()).await;
                let open = status
                    .as_ref()
                    .ok()
                    .and_then(|status| status.as_ref()?.open.as_ref())
                    .filter(|open| open.id == txn.as_str());
                let state = open.map(|open| open.state);
                if state == Some(OpenState::Applied) {
                    return Ok(device);
                }
                device.close().await;
                match (state, &status) {
                    (None, Ok(_)) => {
                        return Err(JobError {
                            code: JobErrorCode::RolledBack,
                            message: "The device rolled the staging config back".to_owned(),
                        });
                    }
                    (Some(_), _) => "the staging config is still reloading".to_owned(),
                    (None, Err(err)) => chain(err),
                }
            }
            Err(err) => chain(&err),
        };
        if Instant::now() + shared.timing.poll >= until {
            return Err(failed(&format!(
                "Cannot log in to the device at {host}: {why}"
            )));
        }
        debug!(%host, %why, "the device isn't ready at its lease yet");
        sleep(shared.timing.poll).await;
    }
}

/// The gateway's login, logged in again once when it drops.
struct Gateway<'a> {
    shared: &'a Shared,
    node: Node,
    remote: Box<dyn Remote>,
}

impl Gateway<'_> {
    /// Runs `request`; with `again`, logs in again and retries once when the login dropped.
    async fn exec(&mut self, request: ExecRequest, again: bool) -> Option<ExecOutput> {
        match self.remote.exec(request.clone()).await {
            Ok(output) => return Some(output),
            Err(err) if !again => {
                debug!(%err, "the gateway's login failed");
                return None;
            }
            Err(_) => {}
        }
        self.reconnect().await;
        self.remote.exec(request).await.ok()
    }

    async fn reconnect(&mut self) {
        if let Ok(remote) = manage::login(self.shared, &self.node).await {
            std::mem::replace(&mut self.remote, remote).close().await;
        }
    }

    async fn text(&mut self, request: ExecRequest) -> Option<String> {
        let what = request.argv.first().cloned().unwrap_or_default();
        let output = self.exec(request, true).await?.check(&what).ok()?;
        String::from_utf8(output.stdout).ok()
    }

    /// Runs a lend command; its answer. Only an idempotent one is retried (`again`).
    async fn lend(&mut self, request: ExecRequest, again: bool) -> Option<Lent> {
        let output = self.exec(request, again).await?.check("lend").ok()?;
        Lent::parse(&String::from_utf8_lossy(&output.stdout))
    }

    async fn leases(&mut self) -> Vec<uniwrt_model::facts::LeaseFacts> {
        let collected = if let Ok(collected) = self.remote.collect(&[Set::Leases]).await {
            collected
        } else {
            self.reconnect().await;
            let Ok(collected) = self.remote.collect(&[Set::Leases]).await else {
                return Vec::new();
            };
            collected
        };
        let mut facts = NodeFacts::default();
        let _ = uniwrt_node::facts::apply(&collected, &mut facts);
        facts.leases
    }
}

async fn tool<T: DeserializeOwned>(
    remote: &dyn Remote,
    command: &txn::Command<T>,
) -> uniwrt_node::Result<T> {
    command.parse(remote.exec(command.request.clone()).await?)
}

fn factory() -> Target {
    Target::new(AdoptionPort::FACTORY.to_string(), 22)
}

fn refused(message: &str) -> JobError {
    JobError {
        code: JobErrorCode::Refused,
        message: message.to_owned(),
    }
}

fn failed(message: &str) -> JobError {
    JobError {
        code: JobErrorCode::NodeFailed,
        message: message.to_owned(),
    }
}
