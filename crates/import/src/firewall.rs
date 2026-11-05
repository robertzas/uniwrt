//! The firewall policy between networks, from the gateway's fw4 zones and forwardings
//! (features/network.md, Firewall policy): each zone takes its network's id as its name (the
//! WAN's is `wan`), and each cell where the forwardings differ from what the networks'
//! purposes seed becomes an override, so the policy renders as the node has it.

use std::collections::BTreeSet;

use uniwrt_model::site::{
    SiteConfig,
    gateway::{ZoneAction, ZonePolicy},
};

use crate::{Source, read};

const WAN: &str = "wan";

pub(crate) fn import(site: &mut SiteConfig, gateway: &Source<'_>, renames: &mut read::Renames) {
    let Some(firewall) = gateway.packages.get("firewall") else {
        return;
    };
    let ids: BTreeSet<&str> = site.networks.iter().map(|n| n.id.as_str()).collect();
    for zone in firewall.sections_of("zone") {
        let Some(name) = zone.option("name") else {
            continue;
        };
        let covers: Vec<&str> = read::items(zone, "network")
            .into_iter()
            .map(|i| renames.network(i))
            .collect();
        let target = match covers.as_slice() {
            [id] if ids.contains(id) => Some((*id).to_owned()),
            nets if !nets.is_empty() && nets.iter().all(|n| n.starts_with("wan")) => {
                Some(WAN.to_owned())
            }
            _ => None,
        };
        if let Some(target) = target.filter(|t| t != name) {
            renames.zones.insert(name.to_owned(), target);
        }
    }
    let zone = |name: &str| -> String {
        renames
            .zones
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_owned())
    };
    let allowed: BTreeSet<(String, String)> = firewall
        .sections_of("forwarding")
        .filter(|f| read::flag(f, "enabled") != Some(false))
        .filter_map(|f| Some((zone(f.option("src")?), zone(f.option("dest")?))))
        .collect();
    let mut zones: Vec<(&str, Option<_>)> = site
        .networks
        .iter()
        .map(|n| (n.id.as_str(), Some(n.purpose)))
        .collect();
    zones.push((WAN, None));
    let mut policies = Vec::new();
    for &(from, purpose) in &zones {
        for &(to, _) in &zones {
            if from == to {
                continue;
            }
            let seeded = purpose.is_some_and(|p| uniwrt_render::seeded(p, to == WAN));
            let actual = allowed.contains(&(from.to_owned(), to.to_owned()));
            if seeded != actual {
                policies.push(ZonePolicy {
                    from: from.to_owned(),
                    to: to.to_owned(),
                    action: if actual {
                        ZoneAction::Allow
                    } else {
                        ZoneAction::Block
                    },
                });
            }
        }
    }
    site.firewall.policies = policies;
}
