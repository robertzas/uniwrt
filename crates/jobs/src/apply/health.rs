//! The health checks an apply verifies each node with (features/lifecycle.md, Render → diff →
//! apply, step 4), relative to a baseline taken while planning (the owner's choice): a check
//! that passed before must pass again, so a network that was already down doesn't block an
//! apply, while what the change itself breaks rolls it back. Reaching the node over a new
//! connection is the first check; the others read the agent's facts and, on the gateway,
//! `nslookup` against its own dnsmasq.

use std::collections::BTreeSet;

use uniwrt_core::MacAddr;
use uniwrt_model::{facts::NodeFacts, node::NodeRole};
use uniwrt_node::{ExecOutput, ExecRequest, proto::ops::Set};

/// The collect sets the checks read.
pub(crate) const SETS: [Set; 3] = [Set::Interfaces, Set::Radios, Set::Batman];

/// The names the gateway resolves: one dnsmasq answers itself (`/etc/hosts`), one it asks
/// upstream for.
pub(crate) const DNS: [&str; 2] = ["localhost", "openwrt.org"];

/// What the checks found on a node.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Health {
    /// The gateway's WAN is up with a default route.
    pub(crate) wan: bool,
    /// The gateway's dnsmasq answers, for itself and from upstream ([`DNS`]).
    pub(crate) dns: [bool; 2],
    /// The radios enabled in the configuration, and those up.
    pub(crate) radios_enabled: BTreeSet<String>,
    pub(crate) radios_up: BTreeSet<String>,
    /// The batman-adv neighbors heard directly.
    pub(crate) mesh_neighbors: BTreeSet<MacAddr>,
}

impl Health {
    pub(crate) fn measure(role: NodeRole, facts: &NodeFacts, dns: [bool; 2]) -> Self {
        let gateway = role == NodeRole::Gateway;
        Self {
            wan: gateway
                && facts.interfaces.iter().any(|iface| {
                    uniwrt_topo::is_wan(iface)
                        && iface.up
                        && (iface.ipv4_gateway.is_some() || iface.ipv6_gateway.is_some())
                }),
            dns: if gateway { dns } else { [false; 2] },
            radios_enabled: facts
                .radios
                .iter()
                .filter(|radio| !radio.disabled)
                .map(|radio| radio.name.clone())
                .collect(),
            radios_up: facts
                .radios
                .iter()
                .filter(|radio| radio.up)
                .map(|radio| radio.name.clone())
                .collect(),
            mesh_neighbors: facts
                .batman
                .iter()
                .flat_map(|mesh| mesh.neighbors.iter().map(|neighbor| neighbor.mac))
                .collect(),
        }
    }

    /// What passed in `before` and fails now, for people; empty when healthy.
    pub(crate) fn regressions(&self, before: &Self) -> Vec<String> {
        let mut failed = Vec::new();
        if before.wan && !self.wan {
            failed.push("the WAN is down".to_owned());
        }
        for (i, name) in DNS.iter().enumerate() {
            if before.dns[i] && !self.dns[i] {
                failed.push(format!("dnsmasq doesn't resolve {name}"));
            }
        }
        // A radio the change disabled is meant to be down.
        let down: Vec<&str> = before
            .radios_up
            .iter()
            .filter(|radio| {
                self.radios_enabled.contains(*radio) && !self.radios_up.contains(*radio)
            })
            .map(String::as_str)
            .collect();
        if !down.is_empty() {
            failed.push(format!(
                "{} down",
                plural_list(&down, "radio is", "radios are")
            ));
        }
        let (had, has) = (before.mesh_neighbors.len(), self.mesh_neighbors.len());
        if has < had {
            failed.push(format!("the mesh hears {has} of its {had} neighbors"));
        }
        failed
    }
}

fn plural_list(items: &[&str], one: &str, many: &str) -> String {
    let noun = if items.len() == 1 { one } else { many };
    let (word, verb) = noun.split_once(' ').unwrap_or((noun, ""));
    format!("{word} {} {verb}", items.join(", "))
}

/// Resolves `name` with the node's own dnsmasq.
pub(crate) fn dns_probe(name: &str) -> ExecRequest {
    ExecRequest::new(["nslookup", name, "127.0.0.1"]).timeout(std::time::Duration::from_secs(10))
}

/// Whether BusyBox's `nslookup` found an answer: it prints a `Name:` line for each.
pub(crate) fn resolved(output: &ExecOutput) -> bool {
    output.status.success()
        && String::from_utf8_lossy(&output.stdout)
            .lines()
            .any(|line| line.starts_with("Name:"))
}

#[cfg(test)]
mod tests {
    use uniwrt_model::{facts::NodeFacts, node::NodeRole};
    use uniwrt_node::{ExecOutput, ExitStatus};

    use super::{Health, resolved};
    use crate::apply::tests::facts::{interface, mesh, radio};

    fn facts(wan_up: bool, radios: &[(&str, bool, bool)], neighbors: u8) -> NodeFacts {
        let mut facts = NodeFacts::default();
        facts
            .interfaces
            .push(interface("wan", "192.0.2.10/24", wan_up, wan_up));
        for &(name, up, disabled) in radios {
            facts.radios.push(radio(name, up, disabled));
        }
        facts.batman.push(mesh(neighbors));
        facts
    }

    #[test]
    fn only_what_passed_before_must_pass_after() {
        let before = Health::measure(
            NodeRole::Gateway,
            &facts(true, &[("radio0", true, false), ("radio1", true, false)], 2),
            [true, false],
        );
        let same = Health::measure(
            NodeRole::Gateway,
            &facts(true, &[("radio0", true, false), ("radio1", true, false)], 3),
            [true, false],
        );
        assert_eq!(same.regressions(&before), Vec::<String>::new());
        let broken = Health::measure(
            NodeRole::Gateway,
            &facts(
                false,
                &[("radio0", false, false), ("radio1", false, true)],
                1,
            ),
            [false, false],
        );
        assert_eq!(
            broken.regressions(&before),
            [
                "the WAN is down",
                "dnsmasq doesn't resolve localhost",
                "radio radio0 is down",
                "the mesh hears 1 of its 2 neighbors",
            ]
        );
        // Down before: not a regression.
        let ap = Health::measure(NodeRole::Ap, &facts(false, &[], 0), [true, true]);
        assert_eq!(ap.dns, [false, false]);
        assert_eq!(ap.regressions(&ap.clone()), Vec::<String>::new());
    }

    #[test]
    fn nslookup_answers_count_only_with_a_name() {
        let out = |code, stdout: &str| ExecOutput {
            status: ExitStatus::Code(code),
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
        };
        assert!(resolved(&out(
            0,
            "Server:\t\t127.0.0.1\nAddress:\t127.0.0.1:53\n\nName:\topenwrt.org\nAddress: 192.0.2.7\n"
        )));
        assert!(!resolved(&out(
            0,
            "Server:\t\t127.0.0.1\n\n** server can't find openwrt.org: NXDOMAIN\n"
        )));
        assert!(!resolved(&out(1, "Name:\tx\n")));
    }
}
