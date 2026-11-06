//! Surveying the managed nodes for an apply: log in to each (with the controller's key, against
//! the pinned host key), read what it holds (packages, services, facts, and on the gateway its
//! DNS), and plan its part. Nodes are reached concurrently; nothing changes on them.

use std::{
    collections::{BTreeMap, BTreeSet},
    net::IpAddr,
};

use futures_util::future::join_all;
use uniwrt_model::{
    apply::{ApplyPlan, PlanAction},
    facts::NodeFacts,
    job::{JobError, JobErrorCode},
    node::{Node, NodeMode, NodeRole},
    site::{SecretRef, SiteConfig, revision::Revision},
};
use uniwrt_node::{
    Credential, ExecRequest, HostKey, Target,
    proto::ops::Set,
    txn::{self, current},
};
use uniwrt_render::Secrets;
use uniwrt_store::{DriftRepo as _, HostKeyRepo as _, VaultRepo as _};
use zeroize::Zeroizing;

use super::{
    health::{self, Health},
    order,
    plan::{self, Part, Report},
};
use crate::{
    Remote,
    adopt::packages,
    errors::{chain, internal},
    runner::Shared,
};

/// The collect sets render reads: without one of them it would render a node it doesn't see
/// (no radios, say, and so no Wi-Fi), so an error there refuses the node.
pub(crate) const RENDER_SETS: [Set; 6] = [
    Set::Board,
    Set::Capabilities,
    Set::Devices,
    Set::Interfaces,
    Set::Phys,
    Set::Radios,
];

/// The services render may switch, which the planner reads.
const SERVICES: [&str; 5] = ["dnsmasq", "odhcpd", "firewall", "sqm", "mwan3"];

/// A login on a node.
pub(crate) struct Login {
    pub(crate) node: Node,
    pub(crate) host_key: HostKey,
    pub(crate) remote: Box<dyn Remote>,
}

/// A managed node in the apply, with its part.
pub(crate) struct Member {
    pub(crate) login: Login,
    pub(crate) baseline: Health,
    pub(crate) part: Part,
}

/// The plan, and the members that carry it out, in its order.
pub(crate) struct Survey {
    pub(crate) plan: ApplyPlan,
    pub(crate) members: Vec<Member>,
}

/// Logs in to `node` at `host` with the controller's key, against its pinned host key.
pub(crate) async fn login(
    shared: &Shared,
    node: &Node,
    host: &str,
) -> Result<(HostKey, Box<dyn Remote>), String> {
    let dial = shared
        .dial
        .as_ref()
        .ok_or_else(|| "This controller cannot reach nodes".to_owned())?;
    let pin = shared
        .store
        .host_key(node.id)
        .await
        .map_err(|err| internal("reading the host key", &err).message)?
        .ok_or_else(|| "No host key is pinned for the node: adopt it again".to_owned())?;
    let host_key = HostKey::from_pin(&pin).map_err(|err| format!("Its pinned host key: {err}"))?;
    let target = Target::new(host.to_owned(), node.port);
    let remote = dial
        .connect(&target, &host_key, &Credential::Key(shared.key.clone()))
        .await
        .map_err(|err| format!("Cannot log in to {target}: {}", chain(&err)))?;
    Ok((host_key, remote))
}

/// Logs in to every node at once; the ones that failed, with why.
pub(crate) async fn connect_all(
    shared: &Shared,
    nodes: Vec<Node>,
) -> (Vec<Login>, Vec<(Node, String)>) {
    let attempts = join_all(nodes.into_iter().map(|node| async move {
        let result = login(shared, &node, &node.host).await;
        (node, result)
    }))
    .await;
    let (mut logins, mut failed) = (Vec::new(), Vec::new());
    for (node, result) in attempts {
        match result {
            Ok((host_key, remote)) => logins.push(Login {
                node,
                host_key,
                remote,
            }),
            Err(why) => failed.push((node, why)),
        }
    }
    (logins, failed)
}

/// What a node holds now.
struct Reading {
    files: BTreeMap<String, Option<String>>,
    services: BTreeMap<String, current::ServiceState>,
    facts: NodeFacts,
    /// The packages installed from the feeds, or why they can't be read.
    installed: Result<BTreeSet<String>, String>,
    dns: [bool; 2],
    /// The controller's address as the gateway sees it.
    controller: Option<IpAddr>,
}

async fn read(shared: &Shared, login: &Login) -> Result<Reading, String> {
    let remote = login.remote.as_ref();
    if !idle_tool(remote).await? {
        return Err("The transaction tool is missing: turn management off and on again".to_owned());
    }
    let packages = current::packages(&plan::packages()).map_err(|err| err.to_string())?;
    let files = remote
        .exec(packages.request.clone())
        .await
        .and_then(|output| packages.parse(output))
        .map_err(|err| format!("Cannot read its configuration: {}", chain(&err)))?;
    let services = current::services(&SERVICES).map_err(|err| err.to_string())?;
    let services = remote
        .exec(services.request.clone())
        .await
        .and_then(|output| services.parse(output))
        .map_err(|err| format!("Cannot read its services: {}", chain(&err)))?;
    let facts = collect(shared, remote, &Set::ALL, &RENDER_SETS).await?;
    let installed = installed(remote, &facts).await;
    let dns = dns(remote, login.node.role).await;
    let controller = if login.node.role == NodeRole::Gateway {
        super::controller::read(remote).await
    } else {
        None
    };
    Ok(Reading {
        files,
        services,
        facts,
        installed,
        dns,
        controller,
    })
}

/// The names of the packages installed, from the package manager's database (empty without
/// one uniwrt knows).
async fn installed(remote: &dyn Remote, facts: &NodeFacts) -> Result<BTreeSet<String>, String> {
    let Some(manager) = facts.capabilities.package_manager else {
        return Ok(BTreeSet::new());
    };
    let mut outputs = Vec::new();
    for argv in packages::commands(manager) {
        let output = remote
            .exec(ExecRequest::new(argv.iter().copied()))
            .await
            .and_then(|output| output.check(argv[0]))
            .map_err(|err| format!("Cannot read its packages: {}", chain(&err)))?;
        outputs.push(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    Ok(packages::parse(manager, &outputs)
        .into_iter()
        .map(|package| package.name)
        .collect())
}

/// Whether the node has the transaction tool; an error when its status can't be read or a
/// transaction is open, whose config is in flux.
pub(crate) async fn idle_tool(remote: &dyn Remote) -> Result<bool, String> {
    let status = txn::status();
    let status = remote
        .exec(status.request.clone())
        .await
        .and_then(|output| status.parse(output))
        .map_err(|err| format!("Cannot read the transaction tool's status: {}", chain(&err)))?;
    match status {
        None => Ok(false),
        Some(txn::Status {
            open: Some(open), ..
        }) => Err(format!(
            "Transaction {} is open on the node ({} s left)",
            open.id, open.remaining_s
        )),
        Some(_) => Ok(true),
    }
}

/// The node's facts from the agent's `sets`; an error in one of the `required` sets fails.
pub(crate) async fn collect(
    shared: &Shared,
    remote: &dyn Remote,
    sets: &[Set],
    required: &[Set],
) -> Result<NodeFacts, String> {
    let collected = remote
        .collect(sets)
        .await
        .map_err(|err| format!("The agent cannot collect its facts: {}", chain(&err)))?;
    let mut facts = NodeFacts::default();
    let errors = uniwrt_node::facts::apply(&collected, &mut facts);
    facts.collected_at = shared.clock.now();
    match errors.into_iter().find(|err| required.contains(&err.set)) {
        Some(err) => Err(format!("Cannot read its facts: {err}")),
        None => Ok(facts),
    }
}

/// Whether the gateway's dnsmasq resolves each of [`health::DNS`]; nothing on other nodes.
pub(crate) async fn dns(remote: &dyn Remote, role: NodeRole) -> [bool; 2] {
    let mut resolved = [false; 2];
    if role == NodeRole::Gateway {
        for (i, name) in health::DNS.iter().enumerate() {
            resolved[i] = remote
                .exec(health::dns_probe(name))
                .await
                .is_ok_and(|output| health::resolved(&output));
        }
    }
    resolved
}

/// The vault's values of the site's secrets, wiped when dropped.
pub(crate) struct VaultSecrets(BTreeMap<SecretRef, Zeroizing<String>>);

impl VaultSecrets {
    /// Adds a value not in the vault yet, such as a password a WAN test tries.
    pub(crate) fn insert(&mut self, secret: SecretRef, value: Zeroizing<String>) {
        self.0.insert(secret, value);
    }
}

impl Secrets for VaultSecrets {
    fn get(&self, secret: &SecretRef) -> Option<&str> {
        self.0.get(secret).map(|value| value.as_str())
    }
}

pub(crate) async fn secrets(shared: &Shared, site: &SiteConfig) -> Result<VaultSecrets, JobError> {
    let mut values = BTreeMap::new();
    for slot in site.secrets() {
        let Some(secret) = slot.value else { continue };
        let value = shared
            .vault
            .secret(secret.name())
            .await
            .map_err(|err| internal("reading the vault", &err))?;
        // A secret missing from the vault is render's issue to report.
        if let Some(value) = value
            && let Ok(text) = std::str::from_utf8(&value)
        {
            values.insert(secret.clone(), Zeroizing::new(text.to_owned()));
        }
    }
    Ok(VaultSecrets(values))
}

/// Reads every logged-in node and plans `revision` for it; `failed` nodes (and those it can't
/// read) are refused, and the nodes in observe mode skipped.
pub(crate) async fn plan_all(
    shared: &Shared,
    revision: &Revision,
    logins: Vec<Login>,
    failed: Vec<(Node, String)>,
    observing: Vec<Node>,
) -> Result<Survey, JobError> {
    let site = &revision.config;
    let secrets = secrets(shared, site).await?;
    let drifts = shared
        .store
        .drifts()
        .await
        .map_err(|err| internal("reading the drift", &err))?;
    let readings = join_all(logins.iter().map(|login| read(shared, login))).await;
    let read: Vec<(&Node, &NodeFacts)> = logins
        .iter()
        .zip(&readings)
        .filter_map(|(login, reading)| Some((&login.node, &reading.as_ref().ok()?.facts)))
        .collect();
    let hops = order::hops(&read);
    let mut plans = Vec::new();
    let mut members = Vec::new();
    for (login, reading) in logins.into_iter().zip(readings) {
        let reading = match reading {
            Ok(reading) => reading,
            Err(why) => {
                plans.push(plan::idle(&login.node, PlanAction::Refused, why));
                login.remote.close().await;
                continue;
            }
        };
        let mut part = plan::plan(
            site,
            &secrets,
            &Report {
                node: &login.node,
                facts: &reading.facts,
                files: &reading.files,
                services: &reading.services,
                installed: reading.installed.as_ref(),
                controller: reading.controller,
            },
            &shared.timing,
        );
        part.plan.hops = hops.get(&login.node.id).copied();
        part.plan.drift = drifts
            .iter()
            .filter(|drift| drift.node == login.node.id)
            .map(|drift| drift.package.clone())
            .collect();
        plans.push(part.plan.clone());
        members.push(Member {
            baseline: Health::measure(login.node.role, &reading.facts, reading.dns),
            login,
            part,
        });
    }
    for (node, why) in failed {
        plans.push(plan::idle(&node, PlanAction::Refused, why));
    }
    for node in observing {
        plans.push(plan::idle(
            &node,
            PlanAction::Skipped,
            "Observe mode: turn management on to apply to it",
        ));
    }
    order::sort(&mut plans);
    let position = |member: &Member| {
        plans
            .iter()
            .position(|plan| plan.node == member.login.node.id)
    };
    members.sort_by_key(|member| position(member));
    Ok(Survey {
        plan: ApplyPlan {
            revision: revision.meta.id,
            nodes: plans,
        },
        members,
    })
}

/// Splits nodes into the managed ones and those in observe mode.
pub(crate) fn managed(nodes: Vec<Node>) -> (Vec<Node>, Vec<Node>) {
    nodes
        .into_iter()
        .partition(|node| node.mode == NodeMode::Managed)
}

/// The job's error when the plan can't apply: every refused node and why.
pub(crate) fn refused(plan: &ApplyPlan) -> JobError {
    let reasons: Vec<String> = plan
        .nodes
        .iter()
        .filter(|node| node.action == PlanAction::Refused)
        .map(|node| {
            let why: Vec<&str> = node
                .reason
                .iter()
                .map(String::as_str)
                .chain(node.issues.iter().map(|issue| issue.message.as_str()))
                .collect();
            format!("{}: {}", node.name, why.join("; "))
        })
        .collect();
    JobError {
        code: JobErrorCode::Refused,
        message: format!("Nothing changed. {}", reasons.join(". ")),
    }
}
