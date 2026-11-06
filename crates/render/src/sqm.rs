//! The `sqm` package (features/wan.md, SQM; D-076): one `queue` that shapes the WAN's layer-3
//! device with cake through sqm-scripts' `piece_of_cake.qos`, a little below the line's rates,
//! with fairness per host behind NAT. With SQM off on a gateway that holds the package, it
//! renders the package without a queue, so nothing is shaped.

use uniwrt_model::site::gateway::{Sqm, WanProto};
use uniwrt_uci::{Package, Section};

use crate::{Requirement, ext::SectionExt, plan::Plan};

/// The packages SQM needs: sqm-scripts with its init script, and cake (a dependency on most
/// releases, named so a release that lacks it gets it).
pub(crate) const REQUIRES: [Requirement; 2] = [
    Requirement {
        package: "sqm-scripts",
        service: Some("sqm"),
    },
    Requirement {
        package: "kmod-sched-cake",
        service: None,
    },
];

/// The queue's section name.
pub(crate) const QUEUE: &str = "wan";

/// The package, or `None` where there is nothing to own: not the gateway, or SQM off and no
/// package on the node.
pub(crate) fn render(plan: &Plan<'_>, held: bool) -> Option<Package> {
    let wan = plan.wan.as_ref()?;
    let sqm = plan.site.wan.sqm.as_ref();
    if sqm.is_none() && !held {
        return None;
    }
    let mut package = Package::new("sqm");
    if let Some(sqm) = sqm {
        package
            .sections
            .push(queue(sqm, &layer3(plan, &wan.device)));
    }
    Some(package)
}

/// The device IPv4 leaves on: PPPoE's and DS-Lite's tunnels have their own.
pub(crate) fn layer3(plan: &Plan<'_>, device: &str) -> String {
    match plan.site.wan.proto {
        WanProto::Pppoe { .. } => "pppoe-wan".to_owned(),
        WanProto::DsLite { .. } => "ds-wan".to_owned(),
        _ => device.to_owned(),
    }
}

fn queue(sqm: &Sqm, device: &str) -> Section {
    Section::named("queue", QUEUE)
        .with("enabled", "1")
        .with("interface", device)
        .with("download", sqm.download_kbps.to_string())
        .with("upload", sqm.upload_kbps.to_string())
        .with("qdisc", "cake")
        .with("script", "piece_of_cake.qos")
        .with(
            "linklayer",
            if sqm.overhead.is_some() {
                "ethernet"
            } else {
                "none"
            },
        )
        .maybe("overhead", sqm.overhead.map(|bytes| bytes.to_string()))
        .with("qdisc_advanced", "1")
        .with("squash_dscp", "1")
        .with("squash_ingress", "1")
        .with("ingress_ecn", "ECN")
        .with("egress_ecn", "NOECN")
        .with("qdisc_really_really_advanced", "1")
        // Fair per host behind NAT: one device's many flows don't crowd the others out.
        .with("iqdisc_opts", "nat dual-dsthost ingress")
        .with("eqdisc_opts", "nat dual-srchost")
        .with("debug_logging", "0")
        .with("verbosity", "5")
}
