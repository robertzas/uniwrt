//! SQM and failover, from the gateway (features/wan.md, SQM and Failover; D-076): the first
//! queue of `sqm` that is on and shapes with cake, and a backup line `wanb` in `network` that
//! mwan3 checks. A queue under another name is renamed `wan`, as render names it; queues that
//! are off stay as they are. The weekly autotune isn't in UCI: it stays as `base` had it.

use std::net::IpAddr;

use uniwrt_model::site::{
    SiteConfig,
    gateway::{BackupLink, BackupProto, Failover, Sqm},
};

use crate::{Source, read};

/// The name render gives the queue.
const QUEUE: &str = "wan";
const WANB: &str = "wanb";

pub(crate) fn import(
    site: &mut SiteConfig,
    base: &SiteConfig,
    gateway: &Source<'_>,
    renames: &mut read::Renames,
) {
    site.wan.sqm = sqm(base, gateway, renames);
    site.wan.failover = failover(gateway);
}

fn sqm(base: &SiteConfig, gateway: &Source<'_>, renames: &mut read::Renames) -> Option<Sqm> {
    let queue = read::sections(gateway.packages, "sqm", "queue").find(|q| {
        q.option("enabled") == Some("1")
            && q.option("qdisc").is_none_or(|qdisc| qdisc == "cake")
            && q.option("interface").is_some()
    })?;
    let rate = |name: &str| queue.option(name)?.parse::<u32>().ok().filter(|&r| r > 0);
    let sqm = Sqm {
        download_kbps: rate("download")?,
        upload_kbps: rate("upload")?,
        overhead: match queue.option("linklayer") {
            Some("ethernet" | "atm") => queue.option("overhead").and_then(|o| o.parse().ok()),
            _ => None,
        },
        autotune: base.wan.sqm.as_ref().is_some_and(|sqm| sqm.autotune),
    };
    if let Some(name) = queue.name.as_deref().filter(|name| *name != QUEUE) {
        renames.queues.insert(name.to_owned(), QUEUE.to_owned());
    }
    Some(sqm)
}

fn failover(gateway: &Source<'_>) -> Option<Failover> {
    let network = gateway.packages.get("network")?;
    let wanb = network.section(WANB).filter(|s| s.kind == "interface")?;
    let tracked = read::sections(gateway.packages, "mwan3", "interface")
        .any(|s| s.name.as_deref() == Some(WANB) && s.option("enabled") != Some("0"));
    if !tracked {
        return None;
    }
    let device = wanb.option("device")?;
    let declared = network
        .sections_of("device")
        .find(|d| d.option("name") == Some(device) && d.option("type") == Some("8021q"));
    let link = match declared {
        Some(vlan) => BackupLink::Port {
            port: vlan.option("ifname")?.to_owned(),
            vlan: vlan.option("vid")?.parse().ok(),
        },
        None if device.starts_with("usb") => BackupLink::Usb {
            device: device.to_owned(),
        },
        None => BackupLink::Port {
            port: device.to_owned(),
            vlan: None,
        },
    };
    let proto = match wanb.option("proto")? {
        "dhcp" => BackupProto::Dhcp,
        "static" => BackupProto::Static {
            address: read::ipv4(wanb)?,
            gateway: wanb.option("gateway")?.parse().ok()?,
            dns: read::items(wanb, "dns")
                .into_iter()
                .filter_map(|d| d.parse::<IpAddr>().ok())
                .collect(),
        },
        _ => return None,
    };
    Some(Failover { link, proto })
}
