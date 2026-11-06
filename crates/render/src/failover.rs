//! Failover to a backup line (features/wan.md, Failover; D-076): `wanb` in the `network` package
//! (on a port taken out of the LAN, a VLAN of one, or a USB tether), and the `mwan3` package,
//! which checks both lines against [`HEALTH_TARGETS`] and routes IPv4 over the backup while the
//! WAN fails. Each line has its own metric, as mwan3 needs. With failover off on a gateway that
//! holds the package, `mwan3` keeps only its globals and the service is off.

use uniwrt_model::site::gateway::{BackupLink, BackupProto, Failover, HEALTH_TARGETS};
use uniwrt_uci::{Package, Section};

use crate::{Requirement, ext::SectionExt, plan::Plan};

/// The backup line's interface.
pub(crate) const WANB: &str = "wanb";
/// The routing metric of the WAN and of the backup: the WAN wins while both are up.
pub(crate) const WAN_METRIC: &str = "10";
const WANB_METRIC: &str = "20";

pub(crate) const REQUIRES: Requirement = Requirement {
    package: "mwan3",
    service: Some("mwan3"),
};

/// The USB network drivers a tethered phone or modem needs.
pub(crate) const USB_REQUIRES: [Requirement; 2] = [
    Requirement {
        package: "kmod-usb-net-rndis",
        service: None,
    },
    Requirement {
        package: "kmod-usb-net-cdc-ether",
        service: None,
    },
];

/// The backup's device as `wanb` uses it, and the 802.1Q device it needs, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Backup {
    pub device: String,
    pub vlan_device: Option<(String, u16)>,
}

impl Backup {
    pub(crate) fn new(failover: &Failover) -> Self {
        match &failover.link {
            BackupLink::Port {
                port,
                vlan: Some(vid),
            } => Self {
                device: format!("{port}.{vid}"),
                vlan_device: Some((port.clone(), *vid)),
            },
            BackupLink::Port { port, vlan: None } => Self {
                device: port.clone(),
                vlan_device: None,
            },
            BackupLink::Usb { device } => Self {
                device: device.clone(),
                vlan_device: None,
            },
        }
    }
}

/// The `network` sections of the backup line: its 802.1Q device, and `wanb`.
pub(crate) fn network(plan: &Plan<'_>) -> Vec<Section> {
    let (Some(backup), Some(failover)) = (&plan.backup, &plan.site.wan.failover) else {
        return Vec::new();
    };
    let device = backup.vlan_device.as_ref().map(|(port, vid)| {
        Section::anonymous("device")
            .with("type", "8021q")
            .with("ifname", port.as_str())
            .with("vid", vid.to_string())
            .with("name", backup.device.as_str())
    });
    let wanb = Section::named("interface", WANB).with("device", backup.device.as_str());
    let wanb = match &failover.proto {
        BackupProto::Dhcp => wanb.with("proto", "dhcp"),
        BackupProto::Static {
            address,
            gateway,
            dns,
        } => wanb
            .with("proto", "static")
            .list("ipaddr", [address.to_string()])
            .with("gateway", gateway.to_string())
            .list("dns", dns.iter().map(ToString::to_string)),
    };
    device
        .into_iter()
        .chain([wanb.with("metric", WANB_METRIC)])
        .collect()
}

/// The `mwan3` package, or `None` where there is nothing to own: not the gateway, or failover
/// off and no package on the node.
pub(crate) fn render(plan: &Plan<'_>, held: bool) -> Option<Package> {
    plan.wan.as_ref()?;
    let on = plan.backup.is_some();
    if !on && !held {
        return None;
    }
    let mut package = Package::new("mwan3");
    package.sections.push(
        Section::named("globals", "globals")
            .with("mmx_mask", "0x3F00")
            .with("logging", "1")
            .with("loglevel", "notice"),
    );
    if on {
        package.sections.extend([
            track("wan"),
            track(WANB),
            member("wan_m1", "wan", "1"),
            member("wanb_m2", WANB, "2"),
            Section::named("policy", "failover")
                .with("use_member", ["wan_m1", "wanb_m2"])
                // Both checks failing (the targets blocked, say) falls back to the routing
                // table rather than to no route at all.
                .with("last_resort", "default"),
            Section::named("rule", "default_v4")
                .with("dest_ip", "0.0.0.0/0")
                .with("family", "ipv4")
                .with("use_policy", "failover"),
        ]);
    }
    Some(package)
}

/// A line's health check: a ping to each target every 5 s; offline after 3 lost in a row with
/// none of the targets answering, online again after 3 answered.
fn track(interface: &str) -> Section {
    Section::named("interface", interface)
        .with("enabled", "1")
        .with("family", "ipv4")
        .with("initial_state", "online")
        .list("track_ip", HEALTH_TARGETS.iter().map(ToString::to_string))
        .with("track_method", "ping")
        .with("reliability", "1")
        .with("count", "1")
        .with("timeout", "2")
        .with("interval", "5")
        .with("failure_interval", "5")
        .with("recovery_interval", "5")
        .with("down", "3")
        .with("up", "3")
}

fn member(name: &str, interface: &str, metric: &str) -> Section {
    Section::named("member", name)
        .with("interface", interface)
        .with("metric", metric)
        .with("weight", "1")
}

/// The networks the WAN's firewall zone covers.
pub(crate) fn zone_networks(plan: &Plan<'_>) -> Vec<&'static str> {
    let mut networks = vec!["wan", "wan6"];
    if plan.backup.is_some() {
        networks.push(WANB);
    }
    networks
}
