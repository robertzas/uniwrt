//! One node's part of an apply, planned from what it reported (pure): render the revision, diff
//! each package against the node's file (D-066: only a semantic change rewrites one), switch
//! the services whose start at boot differs, find where the controller will reach it, and
//! mask secrets for the review screen. On the gateway the part may be the first of several
//! transactions (D-081): when the network this controller sits on moves, the first keeps the
//! old address beside the new one (the bridge) and a [`Round::Settle`] drops it; when the WAN
//! changes among other things, it changes last in a [`Round::Wan`] ([`super::round`]). The
//! later rounds are planned again from what the node holds then.

use std::{
    collections::{BTreeMap, BTreeSet},
    net::Ipv4Addr,
    time::Duration,
};

use uniwrt_model::{
    apply::{AddressChange, NodePlan, PlanAction, PlanIssue, ServiceChange},
    facts::{NodeFacts, PackageManager},
    node::{Node, NodeRole},
    site::{
        SiteConfig,
        node::{ManagedPackage, RawSection},
    },
};
use uniwrt_node::txn::current::ServiceState;
use uniwrt_render::{
    Issue, RENDERABLE, Requirement, Secrets, Target, render, uplink_section, without_wan,
};
use uniwrt_uci::Package;

use super::{
    ApplyTiming, address,
    address::Bridge,
    round::{Round, WAN_PACKAGES, bridge_ports, bridge_round, wan_last},
};

/// What a managed node reported while planning.
pub(crate) struct Report<'a> {
    pub(crate) node: &'a Node,
    pub(crate) facts: &'a NodeFacts,
    /// `/etc/config`, by package; `None` for a package it lacks.
    pub(crate) files: &'a BTreeMap<String, Option<String>>,
    pub(crate) services: &'a BTreeMap<String, ServiceState>,
    /// The packages installed from the feeds, by name, or why they can't be read: only a
    /// configuration that needs one fails on that.
    pub(crate) installed: Result<&'a BTreeSet<String>, &'a String>,
    /// The controller's address as the gateway sees it, for the `uniwrt` record.
    pub(crate) controller: Option<std::net::IpAddr>,
}

/// One node's part, as the job carries it out.
#[derive(Debug, Clone)]
pub(crate) struct Part {
    pub(crate) plan: NodePlan,
    /// The files to install, by package.
    pub(crate) files: Vec<(String, String)>,
    /// Its managed packages as it holds them once applied (installed or kept): drift's
    /// baseline.
    pub(crate) held: BTreeMap<String, String>,
    pub(crate) services: Vec<(String, bool)>,
    /// The feed packages to install before the transaction, with the package manager.
    pub(crate) installs: Option<(PackageManager, Vec<Requirement>)>,
    /// Where the controller reaches it once applied, when that moves.
    pub(crate) address: Option<Ipv4Addr>,
    /// The network the controller sits on moves: this transaction keeps the old address beside
    /// the new one, and the verify waits for the controller to come from the new subnet.
    pub(crate) bridge: Option<Bridge>,
    /// The bridge's ports to bounce once applied, so the hosts behind them ask for a lease.
    pub(crate) bounce: Vec<String>,
    /// The interface whose lease to renew once the gateway applied: an access point that moves.
    pub(crate) renew: Option<String>,
    /// The transactions after this one.
    pub(crate) next: Vec<Round>,
    /// Its dead-man timer, longer when it changes the WAN or the mesh (`uplink`) or bridges.
    pub(crate) timeout: Duration,
    pub(crate) uplink: bool,
}

impl Part {
    /// Nothing to apply, but an address to follow: an access point whose network renumbers
    /// around it.
    pub(crate) fn follows(&self) -> bool {
        self.files.is_empty()
            && self.services.is_empty()
            && self.installs.is_none()
            && self.address.is_some()
    }
}

/// The names of the packages render writes, which the planner reads.
pub(crate) fn packages() -> Vec<&'static str> {
    RENDERABLE.iter().map(|package| package.name()).collect()
}

/// A node's plan with nothing to do: skipped or refused for `reason`.
pub(crate) fn idle(node: &Node, action: PlanAction, reason: impl Into<String>) -> NodePlan {
    NodePlan {
        node: node.id,
        name: node.name.clone(),
        role: node.role,
        action,
        reason: Some(reason.into()),
        packages: Vec::new(),
        drift: Vec::new(),
        raw: Vec::new(),
        services: Vec::new(),
        installs: Vec::new(),
        address: None,
        renumbering: None,
        wan_last: false,
        hops: None,
        timeout_s: 0,
        warnings: Vec::new(),
        issues: Vec::new(),
    }
}

/// The node's packages that parse; an issue for each that doesn't.
fn parsed(
    files: &BTreeMap<String, Option<String>>,
    issues: &mut Vec<PlanIssue>,
) -> BTreeMap<String, uniwrt_uci::Package> {
    let mut current = BTreeMap::new();
    for (name, file) in files {
        let Some(text) = file else { continue };
        match uniwrt_uci::parse(name, text) {
            Ok(package) => {
                current.insert(name.clone(), package);
            }
            Err(err) => issues.push(issue(
                "node",
                format!("The node's /etc/config/{name} doesn't parse: {err}"),
            )),
        }
    }
    current
}

pub(crate) fn plan(
    site: &SiteConfig,
    secrets: &dyn Secrets,
    report: &Report<'_>,
    timing: &ApplyTiming,
) -> Part {
    let node = report.node;
    let mut part = Part {
        plan: idle(node, PlanAction::Unchanged, ""),
        files: Vec::new(),
        held: BTreeMap::new(),
        services: Vec::new(),
        installs: None,
        address: None,
        bridge: None,
        bounce: Vec::new(),
        renew: None,
        next: Vec::new(),
        timeout: timing.timer,
        uplink: false,
    };
    part.plan.reason = None;
    part.plan.raw = raw_overrides(site, node);
    let current = parsed(report.files, &mut part.plan.issues);
    if !part.plan.issues.is_empty() {
        part.plan.action = PlanAction::Refused;
        return part;
    }
    let target = Target {
        node: node.id,
        role: node.role,
        facts: report.facts,
        current: &current,
        controller: report.controller,
    };
    let rendered = match render(site, secrets, target) {
        Ok(rendered) => rendered,
        Err(issues) => {
            part.plan.issues = issues.into_iter().map(from_render).collect();
            part.plan.action = PlanAction::Refused;
            return part;
        }
    };
    part.plan.warnings = rendered.warnings.into_iter().map(from_render).collect();
    let (round, deferred) = first_round(
        site,
        report,
        &rendered.packages,
        &rendered.services,
        &current,
        &mut part,
    );
    let mut uplink = files(&rendered.packages, &round, &current, report, &mut part);
    let coming = installs(&rendered.requires, report, &mut part);
    services(
        &rendered.services,
        report.services,
        &coming,
        &deferred,
        &mut part,
    );
    match address::expected(node, report.facts, site) {
        Ok(address) => {
            part.address = address;
            part.plan.address = address.map(|to| AddressChange {
                from: node.host.clone(),
                to: to.to_string(),
            });
            if address.is_some() && node.role != NodeRole::Gateway {
                // A new lease takes as long as an uplink's change.
                part.renew = uniwrt_render::management_network(site).map(|net| net.id.clone());
                uplink = true;
            }
        }
        // Only a change that applies moves the node.
        Err(why) if !part.files.is_empty() => part.plan.issues.push(issue("node", why)),
        Err(_) => {}
    }
    part.uplink = uplink;
    part.timeout = if uplink {
        timing.timer_uplink
    } else {
        timing.timer
    };
    part.plan.timeout_s = u32::try_from(part.timeout.as_secs()).unwrap_or(u32::MAX);
    part.plan.action = if !part.plan.issues.is_empty() {
        PlanAction::Refused
    } else if part.files.is_empty() && part.services.is_empty() && part.installs.is_none() {
        PlanAction::Unchanged
    } else {
        PlanAction::Change
    };
    if part.plan.action != PlanAction::Change {
        part.next.clear();
    }
    part
}

/// The packages of the node's first transaction, and the services a later one switches: the
/// rendering, unless the gateway's WAN goes last or this controller's network moves (D-081).
fn first_round(
    site: &SiteConfig,
    report: &Report<'_>,
    rendered: &BTreeMap<ManagedPackage, Package>,
    services: &BTreeMap<&'static str, bool>,
    current: &BTreeMap<String, Package>,
    part: &mut Part,
) -> (BTreeMap<ManagedPackage, Package>, BTreeSet<&'static str>) {
    // The services the WAN's transaction switches, when it goes last.
    let mut deferred = BTreeSet::new();

    // This transaction's packages: the rendering, unless the WAN goes last or the controller's
    // network moves (the gateway's, D-081).
    let mut round = rendered.clone();
    if report.node.role == NodeRole::Gateway {
        if wan_last(rendered, current, services, report.services) {
            let empty = Package::new("network");
            let now = current.get("network").unwrap_or(&empty);
            if let Some(network) = round.get_mut(&ManagedPackage::Network) {
                *network = without_wan(network, now);
            }
            for p in WAN_PACKAGES {
                round.remove(&p);
                deferred.insert(p.name());
            }
            part.plan.wan_last = true;
            part.next.push(Round::Wan);
        }
        match report
            .controller
            .map(|controller| address::renumbering(report.facts, site, controller))
            .transpose()
        {
            Ok(bridge) => {
                if let Some(bridge) = bridge.flatten() {
                    bridge_round(&bridge, rendered, &mut round);
                    part.bounce = bridge_ports(rendered);
                    part.plan.renumbering = Some(bridge.plan());
                    part.plan.warnings.push(issue(
                        &format!("networks/{}", bridge.network),
                        format!(
                            "Devices wired to the gateway ask for a new address when its ports \
                             bounce. Devices on Wi-Fi or behind a switch keep their address on \
                             {}, which stops working once the apply is done, until they \
                             reconnect or their lease renews: reconnect them afterwards",
                            bridge.old.trunc()
                        ),
                    ));
                    part.bridge = Some(bridge);
                    part.next.insert(0, Round::Settle);
                }
            }
            Err(why) => part.plan.issues.push(issue("node", why)),
        }
    }
    (round, deferred)
}

/// Fills the review's diffs (the whole change) and this transaction's files; whether the
/// transaction changes the node's uplink.
fn files(
    rendered: &BTreeMap<ManagedPackage, Package>,
    round: &BTreeMap<ManagedPackage, Package>,
    current: &BTreeMap<String, Package>,
    report: &Report<'_>,
    part: &mut Part,
) -> bool {
    // The review's diffs and services are the whole change; the files are this transaction's.
    for (package, after) in rendered {
        let name = package.name();
        let before = current
            .get(name)
            .cloned()
            .unwrap_or_else(|| Package::new(name));
        let mut diff = uniwrt_uci::diff(&before, after);
        if !diff.is_empty() {
            mask(&mut diff);
            part.plan.packages.push(diff);
        }
        if let Some(Some(text)) = report.files.get(name) {
            part.held.insert(name.to_owned(), text.clone());
        }
    }
    let mut uplink = part.bridge.is_some();
    for (package, after) in round {
        let name = package.name();
        let before = current
            .get(name)
            .cloned()
            .unwrap_or_else(|| Package::new(name));
        let diff = uniwrt_uci::diff(&before, after);
        if diff.is_empty() {
            continue;
        }
        uplink |= diff.sections.iter().any(|section| {
            [&section.before, &section.after]
                .into_iter()
                .flatten()
                .filter_map(|id| id.name.as_deref())
                .any(uplink_section)
        });
        // A device only the WAN stands on is its uplink too.
        uplink |= *package == ManagedPackage::Network
            && !uniwrt_uci::diff(&without_wan(after, &before), after).is_empty();
        let file = after.to_file();
        part.held.insert(name.to_owned(), file.clone());
        part.files.push((name.to_owned(), file));
    }
    uplink
}

/// Plans installing the feed packages the configuration `requires` and the node lacks; the
/// services they bring, which the transaction may switch on although absent now.
fn installs(
    requires: &[Requirement],
    report: &Report<'_>,
    part: &mut Part,
) -> BTreeSet<&'static str> {
    let missing: Vec<Requirement> = match report.installed {
        Ok(installed) => requires
            .iter()
            .filter(|r| !installed.contains(r.package))
            .copied()
            .collect(),
        Err(why) => {
            if !requires.is_empty() {
                part.plan.issues.push(issue("node", why.clone()));
            }
            Vec::new()
        }
    };
    if !missing.is_empty() {
        match report.facts.capabilities.package_manager {
            Some(manager) => {
                part.plan.installs = missing.iter().map(|r| r.package.to_owned()).collect();
                part.installs = Some((manager, missing.clone()));
            }
            None => part.plan.issues.push(issue(
                "node",
                format!(
                    "The node lacks {} and has no package manager uniwrt knows to install it",
                    missing
                        .iter()
                        .map(|r| r.package)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            )),
        }
    }
    missing.iter().filter_map(|r| r.service).collect()
}

/// The node's raw sections that render writes: those of the packages it manages.
fn raw_overrides(site: &SiteConfig, node: &Node) -> Vec<RawSection> {
    let Some(config) = site.nodes.iter().find(|config| config.node == node.id) else {
        return Vec::new();
    };
    config
        .raw
        .iter()
        .filter(|raw| {
            config
                .managed
                .iter()
                .any(|package| package.name() == raw.package && RENDERABLE.contains(package))
        })
        .cloned()
        .collect()
}

/// Switches the services whose start at boot differs from what render wants.
/// `coming` are the services of the packages installed first: switched on once they are there.
/// `deferred` are those a later transaction switches: planned, but not part of this one.
fn services(
    wanted: &BTreeMap<&'static str, bool>,
    current: &BTreeMap<String, ServiceState>,
    coming: &BTreeSet<&str>,
    deferred: &BTreeSet<&str>,
    part: &mut Part,
) {
    for (&name, &on) in wanted {
        match (current.get(name), on) {
            (Some(ServiceState::On), true)
            | (Some(ServiceState::Off | ServiceState::Absent) | None, false) => continue,
            (Some(ServiceState::Absent) | None, true) if !coming.contains(name) => {
                part.plan.issues.push(issue(
                    "node",
                    format!("The {name} service isn't installed on the node"),
                ));
                continue;
            }
            _ => {}
        }
        if !deferred.contains(name) {
            part.services.push((name.to_owned(), on));
        }
        part.plan.services.push(ServiceChange {
            name: name.to_owned(),
            enable: on,
        });
    }
}

fn issue(path: &str, message: impl Into<String>) -> PlanIssue {
    PlanIssue {
        path: path.to_owned(),
        message: message.into(),
    }
}

fn from_render(issue: Issue) -> PlanIssue {
    PlanIssue {
        path: issue.path,
        message: issue.message,
    }
}

pub(crate) use uniwrt_model::apply::mask;

#[cfg(test)]
#[path = "plan_tests.rs"]
pub(crate) mod tests;
