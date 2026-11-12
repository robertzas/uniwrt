//! Brownfield import (features/lifecycle.md, Brownfield import; D-072): the configuration a
//! fleet holds, mapped into the site's desired state, so that managing the nodes keeps them as
//! they are.
//!
//! - **API:** [`import`] takes the site to start from, the nodes as [`Source`]s (each its role,
//!   facts and UCI packages) and a way to name new secrets; it gives an [`Import`]: the site
//!   with the networks, WAN, firewall policy, local DNS, WLANs, mesh and radios the nodes hold,
//!   the values of the secrets it found (for the caller to seal), and per node a
//!   [`NodeImport`]: each section's [`Fate`], the residual (what applying the import would
//!   still change), render's issues and a confidence score.
//! - **Method:** the gateway gives the site-wide parts (site settings, networks with their DHCP
//!   options and IPv6 modes, WAN, firewall, DNS), every node its WLANs, radios, mesh and port
//!   profiles, and the first node holding usteer the steering profile. Each node is then rendered with the result
//!   and its sections are paired with the rendered ones, by name or by their type's key
//!   ([`uniwrt_render::raw::key`]), after the renames render's naming needs (`uw_<wlan>_<band>`,
//!   `uw_mesh`, network ids, zones named by their network). A paired section is the model's:
//!   the options render doesn't write are kept as a raw merge, the ones it writes differently
//!   are the residual. An unpaired section is kept whole as a raw section. On access points the
//!   firewall (off, D-067) keeps only its defaults.
//! - **Invariants:** pure: no IO, no clock. Rendering the import gives the node's own packages
//!   up to the renames and the residual (`render(import(x)) ≡ x`, D-072). A config uniwrt
//!   rendered imports with nothing raw and no residual; importing an import's rendering gives the
//!   same site.
//! - **Dependencies:** core, model, render and uci.
//! - **Tests:** `src/tests/`: round trips of the simulated fleet's hand-made configs, OpenWrt's
//!   stock ones, the hwsim lab's gateway and render's golden sites (`cargo nextest run -p
//!   uniwrt-import`).

use std::collections::BTreeMap;

use uniwrt_core::NodeId;
use uniwrt_model::{
    facts::NodeFacts,
    node::NodeRole,
    site::{SecretRef, SiteConfig, node::NodeConfig},
};
use uniwrt_render::Issue;
use uniwrt_uci::{Diff, Package};

mod dhcp;
mod firewall;
mod fit;
mod networks;
mod ports;
mod read;
mod settings;
mod steering;
#[cfg(test)]
mod tests;
mod uplink;
mod wan;
mod wireless;

/// One node to import.
#[derive(Debug, Clone, Copy)]
pub struct Source<'a> {
    pub node: NodeId,
    pub role: NodeRole,
    pub facts: &'a NodeFacts,
    /// Its UCI packages, by name, as read from it.
    pub packages: &'a BTreeMap<String, Package>,
}

/// What the nodes hold, as the site's desired state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    pub site: SiteConfig,
    /// The values of the secrets found (Wi-Fi passphrases, the mesh key, the PPPoE password, and
    /// the options of raw sections that hold secrets, such as WireGuard's keys, D-073), under the
    /// references `site` holds: the caller seals them in the vault.
    pub secrets: BTreeMap<SecretRef, String>,
    pub nodes: Vec<NodeImport>,
}

/// One node's import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeImport {
    pub node: NodeId,
    /// Each of its sections in the packages render writes, in package order.
    pub sections: Vec<SectionImport>,
    /// What applying the import would still change, per package: render's own choices (a
    /// `force` on DHCP pools, the firewall off on an AP) and what the model can't hold.
    pub residual: Vec<Diff>,
    /// Why the node can't render with the import; nothing else is worked out then.
    pub issues: Vec<Issue>,
    pub warnings: Vec<Issue>,
    /// The share of its options the model holds, 0–100: the rest is kept raw.
    pub confidence: u8,
}

/// One section of a node and what became of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionImport {
    pub package: String,
    pub kind: String,
    /// Its name on the node.
    pub name: Option<String>,
    /// The rendered section it became, when render names it differently.
    pub renamed: Option<String>,
    pub fate: Fate,
}

/// What became of a section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fate {
    /// The model holds it; `kept` are its options render doesn't write, kept as a raw merge.
    Mapped { kept: Vec<String> },
    /// Kept whole as a raw section.
    Raw,
    /// Dropped by render's design (the firewall's zones and rules on an access point).
    Dropped,
}

/// Imports the nodes into `base`: the site-wide parts from the gateway among them (else from
/// `base`), WLANs, radios and the mesh from all of them, and each node's raw sections. `mint`
/// names each new secret. Nodes that `base` holds and `sources` doesn't keep their settings.
pub fn import(
    base: &SiteConfig,
    sources: &[Source<'_>],
    mint: &mut dyn FnMut() -> SecretRef,
) -> Import {
    let mut site = base.clone();
    let mut secrets = BTreeMap::new();
    let mut seal = |value: &str| {
        let secret = mint();
        secrets.insert(secret.clone(), value.to_owned());
        secret
    };
    let mut renames = read::Renames::default();
    let gateway = sources.iter().find(|s| s.role == NodeRole::Gateway);
    if let Some(gateway) = gateway {
        settings::import(&mut site, gateway);
        networks::import(&mut site, gateway, &mut renames);
        wan::import(&mut site, gateway, &mut renames, &mut seal);
        uplink::import(&mut site, base, gateway, &mut renames);
        firewall::import(&mut site, gateway, &mut renames);
    }
    wireless::import(&mut site, sources, &renames, &mut seal);
    steering::import(&mut site, sources);
    for source in sources {
        let ports = ports::import(&site, source);
        let config = node_config(&mut site, source.node);
        config.ports = ports;
        config.hostname = read::first(source.packages, "system", "system")
            .and_then(|s| s.option("hostname"))
            .filter(|name| *name != source.facts.board.hostname)
            .map(str::to_owned);
    }
    let nodes = sources
        .iter()
        .map(|source| fit::node(&mut site, (&mut secrets, &mut *mint), source, &renames))
        .collect();
    Import {
        site,
        secrets,
        nodes,
    }
}

/// The node's entry in the site, made if missing.
pub(crate) fn node_config(site: &mut SiteConfig, node: NodeId) -> &mut NodeConfig {
    if let Some(index) = site.nodes.iter().position(|n| n.node == node) {
        return &mut site.nodes[index];
    }
    site.nodes.push(NodeConfig::new(node));
    let last = site.nodes.len() - 1;
    &mut site.nodes[last]
}
