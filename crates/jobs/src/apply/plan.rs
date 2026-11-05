//! One node's part of an apply, planned from what it reported (pure): render the revision, diff
//! each package against the node's file (D-066: only a semantic change rewrites one), switch
//! the services whose start at boot differs, find where the controller will reach it, and
//! mask secrets for the review screen.

use std::{collections::BTreeMap, net::Ipv4Addr, time::Duration};

use uniwrt_model::{
    apply::{AddressChange, MASKED, NodePlan, PlanAction, PlanIssue, ServiceChange},
    facts::NodeFacts,
    node::Node,
    site::SiteConfig,
};
use uniwrt_node::txn::current::ServiceState;
use uniwrt_render::{Issue, RENDERABLE, Secrets, Target, render, uplink_section};
use uniwrt_uci::{Diff, Package, Value};

use super::{ApplyTiming, address};

/// Options that hold secrets in rendered packages (render's contract).
const SECRET_OPTIONS: [&str; 2] = ["key", "password"];

/// What a managed node reported while planning.
pub(crate) struct Report<'a> {
    pub(crate) node: &'a Node,
    pub(crate) facts: &'a NodeFacts,
    /// `/etc/config`, by package; `None` for a package it lacks.
    pub(crate) files: &'a BTreeMap<String, Option<String>>,
    pub(crate) services: &'a BTreeMap<String, ServiceState>,
}

/// One node's part, as the job carries it out.
#[derive(Debug, Clone)]
pub(crate) struct Part {
    pub(crate) plan: NodePlan,
    /// The files to install, by package.
    pub(crate) files: Vec<(String, String)>,
    pub(crate) services: Vec<(String, bool)>,
    /// Where the controller reaches it once applied, when that moves.
    pub(crate) address: Option<Ipv4Addr>,
    /// Its dead-man timer, longer when it changes the WAN or the mesh (`uplink`).
    pub(crate) timeout: Duration,
    pub(crate) uplink: bool,
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
        services: Vec::new(),
        address: None,
        hops: None,
        timeout_s: 0,
        warnings: Vec::new(),
        issues: Vec::new(),
    }
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
        services: Vec::new(),
        address: None,
        timeout: timing.timer,
        uplink: false,
    };
    part.plan.reason = None;
    let mut current = BTreeMap::new();
    for (name, file) in report.files {
        let Some(text) = file else { continue };
        match uniwrt_uci::parse(name, text) {
            Ok(package) => {
                current.insert(name.clone(), package);
            }
            Err(err) => part.plan.issues.push(issue(
                "node",
                format!("The node's /etc/config/{name} doesn't parse: {err}"),
            )),
        }
    }
    if !part.plan.issues.is_empty() {
        part.plan.action = PlanAction::Refused;
        return part;
    }
    let target = Target {
        node: node.id,
        role: node.role,
        facts: report.facts,
        current: &current,
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
    let mut uplink = false;
    for (package, after) in &rendered.packages {
        let name = package.name();
        let before = current
            .get(name)
            .cloned()
            .unwrap_or_else(|| Package::new(name));
        let mut diff = uniwrt_uci::diff(&before, after);
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
        part.files.push((name.to_owned(), after.to_file()));
        mask(&mut diff);
        part.plan.packages.push(diff);
    }
    services(&rendered.services, report.services, &mut part);
    match address::expected(node, report.facts, site) {
        Ok(address) => {
            part.address = address;
            part.plan.address = address.map(|to| AddressChange {
                from: node.host.clone(),
                to: to.to_string(),
            });
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
    } else if part.files.is_empty() && part.services.is_empty() {
        PlanAction::Unchanged
    } else {
        PlanAction::Change
    };
    part
}

/// Switches the services whose start at boot differs from what render wants.
fn services(
    wanted: &BTreeMap<&'static str, bool>,
    current: &BTreeMap<String, ServiceState>,
    part: &mut Part,
) {
    for (&name, &on) in wanted {
        match (current.get(name), on) {
            (Some(ServiceState::On), true)
            | (Some(ServiceState::Off | ServiceState::Absent) | None, false) => {}
            (Some(ServiceState::Absent) | None, true) => part.plan.issues.push(issue(
                "node",
                format!("The {name} service isn't installed on the node"),
            )),
            (Some(_), _) => {
                part.services.push((name.to_owned(), on));
                part.plan.services.push(ServiceChange {
                    name: name.to_owned(),
                    enable: on,
                });
            }
        }
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

/// Hides the values of options that hold secrets; a change still shows as one.
fn mask(diff: &mut Diff) {
    let hide = |value: &mut Option<Value>| match value {
        Some(Value::Single(text)) => MASKED.clone_into(text),
        Some(Value::List(items)) => items.iter_mut().for_each(|item| MASKED.clone_into(item)),
        None => {}
    };
    for section in &mut diff.sections {
        for option in &mut section.options {
            if SECRET_OPTIONS.contains(&option.name.as_str()) {
                hide(&mut option.before);
                hide(&mut option.after);
            }
        }
    }
}

#[cfg(test)]
#[path = "plan_tests.rs"]
pub(crate) mod tests;
