//! The adoption port, from the gateway (features/adoption.md, Path C; D-105): the segment
//! render writes, `interface uw_adopt` on the bridge `br-adopt` with the one port, read back.
//! Anything else stays as the node holds it.

use std::net::Ipv4Addr;

use uniwrt_model::site::{SiteConfig, adoption::AdoptionPort};
use uniwrt_render::{ADOPT_BRIDGE, ADOPT_INTERFACE};

use crate::{Source, read};

pub(crate) fn import(site: &mut SiteConfig, gateway: &Source<'_>) {
    site.adoption_port = adoption_port(gateway);
}

fn adoption_port(gateway: &Source<'_>) -> Option<AdoptionPort> {
    let network = gateway.packages.get("network")?;
    let interface = network
        .section(ADOPT_INTERFACE)
        .filter(|s| s.kind == "interface" && s.option("device") == Some(ADOPT_BRIDGE))?;
    let bridge = network
        .sections_of("device")
        .find(|d| d.option("name") == Some(ADOPT_BRIDGE) && d.option("type") == Some("bridge"))?;
    let [port] = read::items(bridge, "ports")[..] else {
        return None;
    };
    let address: Ipv4Addr = read::ipv4(interface)
        .filter(|net| net.trunc() == AdoptionPort::SUBNET)?
        .addr();
    Some(AdoptionPort {
        port: port.to_owned(),
        address,
    })
}
