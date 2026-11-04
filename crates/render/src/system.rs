//! The `system` package: the node's hostname, time zone and NTP.

use uniwrt_model::{node::NodeRole, site::tz};
use uniwrt_uci::{Package, Section};

use crate::{ext::SectionExt, plan::Plan};

pub(crate) fn render(plan: &Plan<'_>) -> Package {
    let site = &plan.site.site;
    let hostname = plan
        .config
        .hostname
        .clone()
        .unwrap_or_else(|| plan.facts.board.hostname.clone());
    let hostname = if hostname.is_empty() {
        "OpenWrt".to_owned()
    } else {
        hostname
    };
    // Validation refuses unknown zones; UTC keeps a stale document renderable.
    let timezone = tz::posix(&site.timezone).unwrap_or("UTC0");
    let system = Section::anonymous("system")
        .with("hostname", hostname)
        .with("timezone", timezone)
        .with("zonename", site.timezone.as_str())
        .with("ttylogin", "0")
        .with("log_size", "128")
        .with("urandom_seed", "0")
        .maybe(
            "compat_version",
            plan.facts.board.layout.compat_version.as_deref(),
        );
    // The gateway serves time to the network, so APs and clients sync even when the internet
    // is down.
    let serves = plan.role == NodeRole::Gateway;
    let ntp = Section::named("timeserver", "ntp")
        .with("enabled", "1")
        .with("enable_server", if serves { "1" } else { "0" })
        .list("server", site.ntp_servers.iter().map(String::as_str));
    let mut package = Package::new("system");
    package.sections = vec![system, ntp];
    package
}
