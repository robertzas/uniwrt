//! Demo mode's first revision (D-071): the simulated network as the site's document, so that
//! what the demo applies matches what its nodes run: the LAN on the gateway's subnet
//! (192.168.1.0/24 while the fleet fits), the sim's WPA2/WPA3 SSID on both bands and its
//! batman-adv mesh on 5 GHz channel 36, in the sim's country. Every node stays in observe mode and
//! manages nothing until the user chooses (directive 8).

use std::{collections::BTreeMap, net::Ipv4Addr};

use anyhow::Context as _;
use uniwrt_api::FirstSite;
use uniwrt_core::Ipv4Net;
use uniwrt_model::{
    site::{
        SiteConfig,
        network::{Dhcp, Ipv4Config},
        wireless::{Channel, MeshConfig, NodeScope, Wlan, WlanSecurity},
    },
    wifi::Band,
};
use uniwrt_sim::{MESH_KEY, PASSPHRASE, SimNode};
use zeroize::Zeroizing;

/// The 5 GHz channel every simulated node uses.
const MESH_CHANNEL: u16 = 36;

/// The site the simulated `nodes` (the gateway first) describe.
pub(crate) fn first_site(nodes: &[SimNode]) -> anyhow::Result<FirstSite> {
    let hosts = nodes
        .iter()
        .map(|node| node.host.parse::<Ipv4Addr>())
        .collect::<Result<Vec<_>, _>>()
        .context("a simulated node's address is not IPv4")?;
    let gateway = *hosts
        .first()
        .context("the simulated fleet has no gateway")?;
    // The sim widens its LAN to a /16 once the fleet outgrows a /24.
    let prefix = if hosts
        .iter()
        .all(|host| host.octets()[..3] == gateway.octets()[..3])
    {
        24
    } else {
        16
    };
    let lan = Ipv4Net::new(gateway, prefix).context("the LAN's prefix")?;
    let start = u32::from(lan.network()) + 100;
    let end = u32::from(lan.broadcast()) - 6;

    let mut config = SiteConfig::defaults(None);
    "Demo home".clone_into(&mut config.site.name);
    config.site.country = Some("US".to_owned());
    config.addressing.ipv4 = Ipv4Net::new_assert(Ipv4Addr::new(192, 168, 0, 0), 16);
    let network = config
        .networks
        .first_mut()
        .context("the defaults have no LAN")?;
    let lease_time_s = network
        .ipv4
        .as_ref()
        .and_then(|ipv4| ipv4.dhcp.as_ref())
        .map_or(12 * 3600, |dhcp| dhcp.lease_time_s);
    network.ipv4 = Some(Ipv4Config {
        address: lan,
        dhcp: Some(Dhcp {
            start: start.into(),
            end: end.into(),
            lease_time_s,
            reservations: Vec::new(),
        }),
    });
    let network = network.id.clone();
    config.wlans.push(Wlan {
        id: "home".to_owned(),
        ssid: uniwrt_sim::SSID.to_owned(),
        enabled: true,
        network,
        security: WlanSecurity::Wpa2Wpa3,
        passphrase: None,
        bands: vec![Band::G2, Band::G5],
        nodes: NodeScope::All,
        hidden: false,
        isolation: false,
    });
    config.mesh = MeshConfig {
        enabled: true,
        mesh_id: uniwrt_sim::MESH_ID.to_owned(),
        band: Some(Band::G5),
        ..MeshConfig::default()
    };
    // The mesh needs one channel on every node: the sim's.
    for policy in &mut config.radio.bands {
        if policy.band == Band::G5 {
            policy.channel = Channel::Fixed(MESH_CHANNEL);
        }
    }
    let secrets = BTreeMap::from([
        (
            "wlans/home/passphrase".to_owned(),
            Zeroizing::new(PASSPHRASE.to_owned()),
        ),
        ("mesh/key".to_owned(), Zeroizing::new(MESH_KEY.to_owned())),
    ]);
    Ok(FirstSite {
        config,
        message: "The simulated network of demo mode".to_owned(),
        secrets,
    })
}

#[cfg(test)]
mod tests {
    use uniwrt_core::SystemClock;
    use uniwrt_model::site::validate::{Severity, validate};
    use uniwrt_sim::{SimConfig, SimFleet};

    use super::*;

    #[tokio::test]
    async fn the_demo_site_is_the_simulated_network_and_valid() {
        let sim = SimFleet::spawn(&SimConfig::default(), &SystemClock);
        let nodes = sim.nodes();
        let site = first_site(&nodes).unwrap();
        let lan = site.config.networks[0].ipv4.as_ref().unwrap();
        assert_eq!(lan.address.to_string(), "192.168.1.1/24");
        let dhcp = lan.dhcp.as_ref().unwrap();
        assert_eq!(
            (dhcp.start.to_string(), dhcp.end.to_string()),
            ("192.168.1.100".to_owned(), "192.168.1.249".to_owned())
        );
        assert_eq!(site.config.wlans[0].ssid, uniwrt_sim::SSID);
        assert!(site.config.mesh.enabled);
        // Valid once its secrets are sealed (the API checks it again then).
        let issues = validate(&site.config, &std::collections::BTreeSet::new());
        let errors: Vec<_> = issues
            .iter()
            .filter(|issue| issue.severity == Severity::Error)
            .filter(|issue| !issue.path.ends_with("passphrase") && issue.path != "mesh/key")
            .collect();
        assert!(errors.is_empty(), "{errors:?}");
    }
}
