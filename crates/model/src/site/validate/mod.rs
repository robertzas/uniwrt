//! Validating a [`SiteConfig`]: errors refuse a save, warnings are shown with it. Paths match the
//! [`diff`](super::diff) paths joined by `/`, such as `networks/iot/ipv4/dhcp/start`.
//!
//! The address plan's guarantees (features/network.md, IPAM): subnets do not overlap, gateways
//! and pools sit inside their subnet, reservations are unique, nothing collides with CGNAT
//! (100.64.0.0/10) or the WAN, and 192.168.1.0/24 stays free for factory-default devices.

mod gateway;
mod names;
mod network;
mod nodes;
mod wireless;

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use uniwrt_core::NodeId;

use super::{SCHEMA_VERSION, SiteConfig};

pub use names::{is_hostname, is_id};
pub use uniwrt_uci::{
    is_name as is_uci_name, is_package as is_uci_package, is_type as is_uci_type,
};

/// A problem found in a document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Issue {
    /// Where, such as `wlans/home/passphrase`.
    pub path: String,
    pub severity: Severity,
    /// For people: what is wrong and, when it helps, what to do.
    pub message: String,
}

/// How bad an [`Issue`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// The document cannot be saved.
    Error,
    /// It can be saved, but probably not as meant.
    Warning,
}

/// What the checks found so far.
#[derive(Default)]
pub(super) struct Issues(Vec<Issue>);

impl Issues {
    fn error(&mut self, path: impl Into<String>, message: impl Into<String>) {
        self.push(path.into(), Severity::Error, message.into());
    }

    fn warn(&mut self, path: impl Into<String>, message: impl Into<String>) {
        self.push(path.into(), Severity::Warning, message.into());
    }

    fn push(&mut self, path: String, severity: Severity, message: String) {
        self.0.push(Issue {
            path,
            severity,
            message,
        });
    }
}

/// Every issue of `config`. `nodes` are the adopted nodes, which overrides and scopes may name.
pub fn validate(config: &SiteConfig, nodes: &BTreeSet<NodeId>) -> Vec<Issue> {
    let mut issues = Issues::default();
    if config.schema != SCHEMA_VERSION {
        issues.error(
            "schema",
            format!(
                "Schema {} is not this uniwrt's ({SCHEMA_VERSION})",
                config.schema
            ),
        );
    }
    site(config, &mut issues);
    network::addressing(config, &mut issues);
    network::networks(config, &mut issues);
    wireless::wlans(config, nodes, &mut issues);
    wireless::mesh(config, &mut issues);
    wireless::radio(config, &mut issues);
    gateway::wan(config, &mut issues);
    gateway::firewall(config, &mut issues);
    gateway::services(config, &mut issues);
    nodes::nodes(config, nodes, &mut issues);
    issues.0
}

/// Whether any of `issues` refuses a save.
pub fn has_errors(issues: &[Issue]) -> bool {
    issues.iter().any(|issue| issue.severity == Severity::Error)
}

fn site(config: &SiteConfig, issues: &mut Issues) {
    let site = &config.site;
    let name = site.name.trim();
    if name.is_empty() || name.chars().count() > 64 {
        issues.error("site/name", "The site needs a name of up to 64 characters");
    }
    match &site.country {
        Some(country)
            if !(country.len() == 2 && country.bytes().all(|b| b.is_ascii_uppercase())) =>
        {
            issues.error(
                "site/country",
                "The country is a two-letter ISO 3166 code, such as US or DE",
            );
        }
        None if config.wlans.iter().any(|wlan| wlan.enabled) || config.mesh.enabled => {
            issues.warn(
                "site/country",
                "Set the country: the radios need it for their regulatory domain",
            );
        }
        _ => {}
    }
    let timezone = &site.timezone;
    if timezone.is_empty()
        || timezone.len() > 64
        || !timezone
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/_+-".contains(&b))
    {
        issues.error(
            "site/timezone",
            "The time zone is an IANA name, such as Europe/Berlin or UTC",
        );
    }
    if !names::is_dns_name(&site.domain) {
        issues.error(
            "site/domain",
            "The domain is a DNS name, such as lan or home.arpa",
        );
    }
    if site.ntp_servers.len() > 8 {
        issues.error("site/ntp_servers", "At most 8 NTP servers");
    }
    for (i, server) in site.ntp_servers.iter().enumerate() {
        if server.parse::<std::net::IpAddr>().is_err() && !names::is_dns_name(server) {
            issues.error(
                format!("site/ntp_servers/{i}"),
                format!("{server:?} is neither a host name nor an IP address"),
            );
        }
    }
}
