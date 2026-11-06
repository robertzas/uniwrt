//! Port profiles: which networks a node's Ethernet ports carry. Whether the node has the port
//! is render's to say, as only its facts know.

use std::collections::BTreeSet;

use super::Issues;
use crate::site::{
    SiteConfig,
    node::{NodeConfig, TaggedNetworks},
};

pub(super) fn ports(config: &SiteConfig, node: &NodeConfig, at: &str, issues: &mut Issues) {
    let mut seen = BTreeSet::new();
    for profile in &node.ports {
        let at = format!("{at}/ports/{}", profile.port);
        if !is_port(&profile.port) {
            issues.error(
                &at,
                format!(
                    "{:?} is not a port: a device such as lan2, or a switch port such as \
                     switch0:3",
                    profile.port
                ),
            );
        }
        if !seen.insert(profile.port.as_str()) {
            issues.error(&at, format!("Port {} has two profiles", profile.port));
        }
        if let Some(native) = &profile.native
            && !config.networks.iter().any(|n| &n.id == native)
        {
            issues.error(format!("{at}/native"), format!("No network is {native}"));
        }
        let TaggedNetworks::Only { networks } = &profile.tagged else {
            continue;
        };
        let mut listed = BTreeSet::new();
        for id in networks {
            let path = format!("{at}/tagged");
            match config.networks.iter().find(|n| &n.id == id) {
                None => issues.error(path, format!("No network is {id}")),
                Some(network) if network.vlan.is_none() => issues.error(
                    path,
                    format!(
                        "{} has no VLAN, so a port carries it only untagged",
                        network.name
                    ),
                ),
                Some(_) if profile.native.as_ref() == Some(id) => {
                    issues.error(path, format!("{id} is the port's untagged network already"));
                }
                Some(_) if !listed.insert(id.as_str()) => {
                    issues.error(path, format!("{id} is listed twice"));
                }
                Some(_) => {}
            }
        }
    }
}

/// A port's name: a network device's (`lan2`, `eth1`), or a switch's port number
/// (`switch0:3`).
fn is_port(name: &str) -> bool {
    let device = |s: &str| {
        !s.is_empty()
            && s.len() <= 15
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'@'))
    };
    match name.split_once(':') {
        Some((switch, number)) => device(switch) && number.parse::<u8>().is_ok(),
        None => device(name),
    }
}

#[cfg(test)]
mod tests {
    use super::is_port;

    #[test]
    fn port_names() {
        assert!(is_port("lan2") && is_port("eth0") && is_port("switch0:3"));
        assert!(!is_port("") && !is_port("switch0:x") && !is_port("lan 2"));
        assert!(!is_port("a-very-long-device-name"));
    }
}
