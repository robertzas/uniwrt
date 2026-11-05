//! Demo mode's first revision (D-071, D-073): the brownfield import of the simulated nodes it
//! adopted (their facts and `/etc/config` as they hold them, which `POST v1/import/preview`
//! reads too) into the defaults, named "Demo home". What the demo applies then matches what its
//! nodes run, up to the import's residual: the LAN on the gateway's subnet, the sim's WPA2/WPA3
//! SSID on both bands, its batman-adv mesh on 5 GHz, its country. Every node stays in observe
//! mode and manages nothing until the user chooses (directive 8).

use std::collections::BTreeMap;

use anyhow::{Context as _, bail};
use uniwrt_api::FirstSite;
use uniwrt_core::NodeId;
use uniwrt_import::Source;
use uniwrt_model::site::{SecretRef, SiteConfig, revision::RevisionKind};
use uniwrt_sim::Brownfield;
use uniwrt_uci::Package;
use zeroize::Zeroizing;

/// The site the simulated nodes hold: `found` in the sim's order, the first of them `adopted`
/// under these ids.
pub(crate) fn first_site(found: &[Brownfield], adopted: &[NodeId]) -> anyhow::Result<FirstSite> {
    let packages = found
        .iter()
        .take(adopted.len())
        .map(|node| {
            node.files
                .iter()
                .map(|(name, text)| Ok((name.clone(), uniwrt_uci::parse(name, text)?)))
                .collect::<Result<BTreeMap<String, Package>, uniwrt_uci::ParseError>>()
                .with_context(|| format!("{}'s /etc/config doesn't parse", node.name))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let sources: Vec<Source<'_>> = adopted
        .iter()
        .zip(found)
        .zip(&packages)
        .map(|((&id, node), packages)| Source {
            node: id,
            role: node.role,
            facts: &node.facts,
            packages,
        })
        .collect();
    let mut base = SiteConfig::defaults(None);
    "Demo home".clone_into(&mut base.site.name);
    let mut n = 0_u128;
    let mut mint = || {
        n += 1;
        SecretRef::numbered(n)
    };
    let import = uniwrt_import::import(&base, &sources, &mut mint);
    if let Some(node) = import.nodes.iter().find(|node| !node.issues.is_empty()) {
        bail!(
            "simulated node {} doesn't import: {:?}",
            node.node,
            node.issues
        );
    }
    Ok(FirstSite {
        config: import.site,
        kind: RevisionKind::Import,
        message: "Imported from the simulated network of demo mode".to_owned(),
        secrets: import
            .secrets
            .into_iter()
            .map(|(secret, value)| (secret, Zeroizing::new(value)))
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use uniwrt_core::SystemClock;
    use uniwrt_model::{
        site::validate::{Severity, validate},
        wifi::Band,
    };
    use uniwrt_sim::{SimConfig, SimFleet};

    use super::*;

    #[tokio::test]
    async fn the_demo_site_is_the_simulated_network_imported_and_valid() {
        let sim = SimFleet::spawn(&SimConfig::default(), &SystemClock);
        let found = sim.brownfield();
        let ids: Vec<NodeId> = (1..found.len())
            .map(|n| NodeId::new(i64::try_from(n).unwrap()))
            .collect();
        let site = first_site(&found, &ids).unwrap();
        let config = &site.config;
        assert_eq!(config.site.name, "Demo home");
        let lan = config.networks[0].ipv4.as_ref().unwrap();
        assert_eq!(lan.address.to_string(), "192.168.1.1/24");
        let dhcp = lan.dhcp.as_ref().unwrap();
        assert_eq!(
            (dhcp.start.to_string(), dhcp.end.to_string()),
            ("192.168.1.100".to_owned(), "192.168.1.249".to_owned())
        );
        assert_eq!(config.wlans[0].ssid, uniwrt_sim::SSID);
        assert!(config.mesh.enabled);
        assert_eq!(config.mesh.band, Some(Band::G5));
        let values: Vec<&str> = site.secrets.values().map(|v| v.as_str()).collect();
        assert!(values.contains(&uniwrt_sim::PASSPHRASE) && values.contains(&uniwrt_sim::MESH_KEY));
        assert!(config.nodes.iter().all(|node| node.managed.is_empty()));
        let adopted = ids.iter().copied().collect();
        let errors: Vec<_> = validate(config, &adopted)
            .into_iter()
            .filter(|issue| issue.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "{errors:?}");
    }
}
