//! The `dropbear` package (features/security.md, Node hardening; D-150): the node's own file,
//! its SSH servers hardened as the node's `ssh` says. Render owns three options of every
//! `dropbear` section: `PasswordAuth` and `RootPasswordAuth` (`off` for key-only SSH, else
//! OpenWrt's `on`) and `DirectInterface`, the management network's interface for SSH there
//! alone (dropbear binds that device, so no other network reaches it, not even through the
//! node's address there), else none. The rest (ports, banners, keys, other instances) stays the
//! node's. A node without the file gets OpenWrt's `main` section while any of it is on, and
//! nothing otherwise.
//!
//! The gateway is bound only while this controller logs in to it from inside the management
//! network's subnet ([`Target::controller`](crate::Target::controller)): from anywhere else its
//! packets arrive on another device, which the bound server ignores. Where the gateway didn't say
//! (a preview, an import), it is bound, and the apply, which always reads it, checks; its verify
//! logs in anew to the server that runs then before it confirms. An AP holds an address only on
//! the management network, so whatever route reaches it arrives there.

use std::net::IpAddr;

use uniwrt_model::{facts::SshServerFacts, node::NodeRole};
use uniwrt_uci::{Package, Section};

use crate::{Issue, plan::Plan};

const PACKAGE: &str = "dropbear";
const SECTION: &str = "dropbear";

pub(crate) fn render(plan: &mut Plan<'_>, current: Option<&Package>) -> Option<Package> {
    let ssh = plan.config.ssh;
    let held = current.filter(|held| held.sections_of(SECTION).next().is_some());
    if held.is_none() && !ssh.any() {
        return None;
    }
    let bound = ssh.management_only && reachable(plan);
    let mut package = held.cloned().unwrap_or_else(stock);
    let on_off = |off: bool| if off { "off" } else { "on" };
    for section in package.sections.iter_mut().filter(|s| s.kind == SECTION) {
        section.set("PasswordAuth", on_off(ssh.key_only));
        section.set("RootPasswordAuth", on_off(ssh.key_only));
        if bound {
            section.set("DirectInterface", plan.mgmt);
        } else {
            section.remove("DirectInterface");
        }
    }
    Some(package)
}

/// OpenWrt's own file, without its commented-out options.
fn stock() -> Package {
    let mut package = Package::new(PACKAGE);
    package.sections = vec![
        Section::named(SECTION, "main")
            .with("enable", "1")
            .with("PasswordAuth", "on")
            .with("RootPasswordAuth", "on")
            .with("Port", "22"),
    ];
    package
}

/// Whether this controller still reaches the node once SSH answers on the management network
/// alone; an issue where it wouldn't.
fn reachable(plan: &mut Plan<'_>) -> bool {
    if plan.role != NodeRole::Gateway {
        return true;
    }
    let at = format!("nodes/{}/ssh/management_only", plan.config.node);
    let Some(network) = plan.site.networks.iter().find(|n| n.id == plan.mgmt) else {
        plan.errors
            .push(Issue::new(at, "The site has no management network"));
        return false;
    };
    let Some(subnet) = network.ipv4.as_ref().map(|ipv4| ipv4.address.trunc()) else {
        plan.errors.push(Issue::new(
            at,
            format!(
                "{} has no IPv4 subnet, so nothing tells whether this controller logs in from it",
                network.name
            ),
        ));
        return false;
    };
    match plan.controller {
        None => true,
        Some(IpAddr::V4(ip)) if subnet.contains(&ip) => true,
        Some(from) => {
            plan.errors.push(Issue::new(
                at,
                format!(
                    "This controller logs in to the gateway from {from}, outside {} ({subnet}): \
                     SSH bound to that network would shut it out. Move the controller into \
                     {0}, or let SSH answer on every network",
                    network.name
                ),
            ));
            false
        }
    }
}

/// What a rendered `dropbear` file asks of the servers that run it: whether passwords and
/// root's password log in, and whether they are bound to a device. The apply checks the running
/// servers against it before it confirms (D-150).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SshServers {
    pub password: bool,
    pub root_password: bool,
    pub bound: bool,
}

impl SshServers {
    /// What `package`'s first enabled `dropbear` section asks (render sets them all alike);
    /// `None` without one.
    pub fn of(package: &Package) -> Option<Self> {
        let section = package
            .sections_of(SECTION)
            .find(|s| flag(s.option("enable"), true))?;
        Some(Self {
            password: flag(section.option("PasswordAuth"), true),
            root_password: flag(section.option("RootPasswordAuth"), true),
            bound: section
                .option("DirectInterface")
                .is_some_and(|i| !i.is_empty()),
        })
    }

    /// Why the servers procd runs don't run as asked, if they don't.
    pub fn check(self, servers: Option<&[SshServerFacts]>) -> Result<(), String> {
        let servers = servers.ok_or("procd has no dropbear service")?;
        let running: Vec<_> = servers.iter().filter(|s| s.running).collect();
        if running.is_empty() {
            return Err("No SSH server runs".to_owned());
        }
        for server in running {
            if server.password != self.password || server.root_password != self.root_password {
                return Err(format!(
                    "An SSH server still runs with password logins {}",
                    if server.password { "on" } else { "off" }
                ));
            }
            if server.device.is_some() != self.bound {
                return Err(match &server.device {
                    Some(device) => format!("An SSH server is still bound to {device}"),
                    None => "An SSH server still listens on every network".to_owned(),
                });
            }
        }
        Ok(())
    }
}

/// A UCI boolean as OpenWrt's validation reads it; `default` when unset or unknown.
fn flag(value: Option<&str>, default: bool) -> bool {
    match value {
        Some("1" | "on" | "yes" | "true" | "enabled") => true,
        Some("0" | "off" | "no" | "false" | "disabled") => false,
        _ => default,
    }
}
