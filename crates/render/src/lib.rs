//! Render (features/lifecycle.md, Render → diff → apply): a site's desired state and one node's
//! facts become the UCI packages the controller owns on that node.
//!
//! - **API:** [`render`] takes the [`SiteConfig`], the vault's values of its secrets
//!   ([`Secrets`]) and a [`Target`]: the node, its role, its [`NodeFacts`] and the packages it
//!   holds now. It gives a [`Rendered`]: one [`Package`] per managed package (D-011), the
//!   services to enable or disable, and warnings; or every [`Issue`] that keeps the node from
//!   being rendered. Packages render in the order `system`, `network`, `wireless`, `dhcp`,
//!   `firewall`; the node's raw sections are merged last ([`raw`](mod@raw)).
//! - **Shapes:** every node bridges all networks into `br-lan`. With only an untagged network
//!   that is a plain bridge, as OpenWrt ships; with VLANs it filters VLANs (`bridge-vlan`), a
//!   network's interface sits on `br-lan.<vid>`, Ethernet ports and `bat0` are trunks (the
//!   untagged network untagged, the others tagged), and Wi-Fi joins through its interface. A
//!   swconfig switch maps each VLAN to the CPU port, whose `<cpu>.<vid>` devices join the
//!   bridge as access ports. The gateway routes: a static interface per network, the WAN,
//!   dnsmasq and odhcpd, and a firewall zone per network seeded by its purpose. APs bridge
//!   only: a DHCP client on the management network, and dnsmasq, odhcpd and the firewall off.
//! - **Invariants:** pure: no IO, no clock; the same input gives the same packages. Rendering
//!   reads capabilities (switch kind, hostapd features, batman-adv), never release versions.
//!   Values the node generated for itself (its DHCP DUID, LED sections, odhcpd's paths) are
//!   carried over from its current packages ([`preserve`](mod@preserve)). Every package
//!   [`Package::validate`]s. Secrets appear only in option values (`key`, `password`).
//! - **Dependencies:** core, model and uci.
//! - **Tests:** unit tests beside each part; `tests/golden.rs` renders each role × feature ×
//!   capability set into `tests/snapshots/` (insta; review changes, never accept them blindly).

use std::collections::BTreeMap;

use uniwrt_core::NodeId;
use uniwrt_model::{
    facts::NodeFacts,
    node::NodeRole,
    site::{
        SecretRef, SiteConfig,
        node::{ManagedPackage, NodeConfig},
    },
};
use uniwrt_uci::Package;

mod dhcp;
mod ext;
mod firewall;
mod mesh;
mod network;
mod plan;
mod preserve;
mod raw;
mod switch;
mod system;
mod wan;
mod wireless;

/// The vault's values of the site's secrets.
pub trait Secrets {
    fn get(&self, secret: &SecretRef) -> Option<&str>;
}

impl Secrets for BTreeMap<SecretRef, String> {
    fn get(&self, secret: &SecretRef) -> Option<&str> {
        BTreeMap::get(self, secret).map(String::as_str)
    }
}

/// The node to render for.
#[derive(Debug, Clone, Copy)]
pub struct Target<'a> {
    pub node: NodeId,
    pub role: NodeRole,
    pub facts: &'a NodeFacts,
    /// The packages the node holds now, read right before rendering, by name; the source of
    /// the values it generated for itself. Empty when unknown.
    pub current: &'a BTreeMap<String, Package>,
}

/// What a node is given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    /// The managed packages (D-011); the node's other packages are never touched.
    pub packages: BTreeMap<ManagedPackage, Package>,
    /// Init scripts to enable (`true`) or disable, such as `dnsmasq` on an AP.
    pub services: BTreeMap<&'static str, bool>,
    pub warnings: Vec<Issue>,
}

/// Something about the node or the site that render reports, at the path of the document's
/// part it concerns (`wlans/home`, `mesh`) or of the node (`node`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub path: String,
    pub message: String,
}

impl Issue {
    pub(crate) fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
        }
    }
}

/// The packages render can write; the other [`ManagedPackage`]s get renderers in later work.
pub const RENDERABLE: [ManagedPackage; 5] = [
    ManagedPackage::System,
    ManagedPackage::Network,
    ManagedPackage::Wireless,
    ManagedPackage::Dhcp,
    ManagedPackage::Firewall,
];

/// Renders the node's managed packages, or says why it can't.
pub fn render(
    site: &SiteConfig,
    secrets: &dyn Secrets,
    target: Target<'_>,
) -> Result<Rendered, Vec<Issue>> {
    let default_config = NodeConfig::new(target.node);
    let config = site
        .nodes
        .iter()
        .find(|n| n.node == target.node)
        .unwrap_or(&default_config);
    let mut errors: Vec<Issue> = config
        .managed
        .iter()
        .filter(|package| !RENDERABLE.contains(package))
        .map(|package| {
            Issue::new(
                "node",
                format!("uniwrt can't render the {} package yet", package.name()),
            )
        })
        .collect();
    let mut plan = match plan::Plan::new(site, secrets, config, target) {
        Ok(plan) => plan,
        Err(mut issues) => {
            errors.append(&mut issues);
            return Err(errors);
        }
    };
    let mut packages = BTreeMap::new();
    let mut services = BTreeMap::new();
    for package in config.managed.iter().filter(|p| RENDERABLE.contains(p)) {
        let rendered = match package {
            ManagedPackage::System => system::render(&plan),
            ManagedPackage::Network => network::render(&plan),
            ManagedPackage::Wireless => wireless::render(&mut plan),
            ManagedPackage::Dhcp => {
                let routes = plan.role == NodeRole::Gateway;
                services.insert("dnsmasq", routes);
                services.insert("odhcpd", routes);
                dhcp::render(&plan)
            }
            ManagedPackage::Firewall => {
                services.insert("firewall", plan.role == NodeRole::Gateway);
                firewall::render(&plan)
            }
            _ => continue,
        };
        let mut rendered = rendered;
        preserve::carry_over(&mut rendered, target.current);
        raw::merge(&mut rendered, &config.raw);
        if let Err(invalid) = rendered.validate() {
            errors.push(Issue::new(
                "node",
                format!("The {} package came out invalid: {invalid}", rendered.name),
            ));
        }
        packages.insert(*package, rendered);
    }
    errors.append(&mut plan.errors);
    if errors.is_empty() {
        Ok(Rendered {
            packages,
            services,
            warnings: plan.warnings,
        })
    } else {
        Err(errors)
    }
}
