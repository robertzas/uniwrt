//! Render (features/lifecycle.md, Render → diff → apply): a site's desired state and one node's
//! facts become the UCI packages the controller owns on that node.
//!
//! - **API:** [`render`] takes the [`SiteConfig`], the vault's values of its secrets
//!   ([`Secrets`]) and a [`Target`]: the node, its role, its [`NodeFacts`] and the packages it
//!   holds now. It gives a [`Rendered`]: one [`Package`] per managed package (D-011), the
//!   services to enable or disable, and warnings; or every [`Issue`] that keeps the node from
//!   being rendered, and the feed packages it needs ([`Requirement`]). Packages render in the
//!   order `system`, `network`, `wireless`, `dhcp`, `firewall`, `usteer` (only with steering
//!   on and a steered WLAN on the node, or the node holding it), `sqm`, `mwan3`, `upnpd`,
//!   `https-dns-proxy`, `ddns`, `mdns_repeater` (the last six only on the gateway, only with SQM,
//!   failover, UPnP, DoH, DDNS names or mDNS reflection on, or the node holding them); the node's raw sections are merged last ([`raw`](mod@raw)), whose [`raw::key`]
//!   says how an anonymous section is found. A package render doesn't write that a raw section
//!   names is patched: the node's own file in [`Target::current`] with its raw sections
//!   ([`Rendered::patched`], D-143).
//! - **Shapes:** every node bridges all networks into `br-lan`. With only an untagged network
//!   that is a plain bridge, as OpenWrt ships; with VLANs it filters VLANs (`bridge-vlan`), a
//!   network's interface sits on `br-lan.<vid>`, `bat0` is a trunk (the untagged network
//!   untagged, the others tagged), Ethernet ports are trunks unless the node's port profiles
//!   say otherwise (a port that carries nothing leaves the bridge), and Wi-Fi joins through its
//!   interface. A swconfig switch maps each VLAN to the CPU port, whose `<cpu>.<vid>` devices
//!   join the bridge as access ports, and its user ports per their profiles. The gateway
//!   routes: a static interface per network, the WAN, dnsmasq (pools with their options and
//!   network boot, the gateway as NTP server where a pool names none and it serves time, the
//!   upstream DNS: the ISP's, custom servers or DoH's local instances, DNS filtering's conf
//!   directory and, for clients that bypass it, an unfiltered instance the firewall redirects
//!   their DNS to ([`blocking`](mod@blocking), D-133), and `uniwrt.<domain>`
//!   for the controller when [`Target::controller`] sits in a network) and odhcpd (each network's IPv6 clients mode), and a firewall zone per network
//!   seeded by its purpose, port forwards and the DMZ host as `redirect`s, and miniupnpd for
//!   the networks UPnP serves, and the WireGuard server ([`wireguard`](mod@wireguard): the `vpn`
//!   interface with its peers, its zone and the WAN's rule) and Tailscale ([`tailscale`](mod@tailscale):
//!   its zone and the WAN's rule; D-140); a network overlapping the subnet its uplink holds now is an issue.
//!   APs bridge only: a DHCP client on the management network, and dnsmasq, odhcpd and the
//!   firewall off.
//! - **Staging** ([`staging`], D-107): a factory-default device's packages made safe on any LAN
//!   (a DHCP client on its bridge, no DHCP server, Wi-Fi off, the WAN bridged), the
//!   first transaction of an adoption through the adoption port.
//! - **Invariants:** pure: no IO, no clock; the same input gives the same packages. Rendering
//!   reads capabilities (switch kind, hostapd features, batman-adv), never release versions.
//!   Values the node generated for itself (its DHCP DUID, LED sections, odhcpd's paths) are
//!   carried over from its current packages ([`preserve`](mod@preserve)). Every package
//!   [`Package::validate`]s. Secrets appear only in option values (`key`, `password`).
//! - **Dependencies:** core, model and uci.
//! - **Tests:** unit tests beside each part; `tests/golden.rs` renders each role × feature ×
//!   capability set into `tests/snapshots/` (insta; review changes, never accept them blindly).

use std::{collections::BTreeMap, net::IpAddr};

use uniwrt_core::NodeId;
use uniwrt_model::{
    facts::NodeFacts,
    node::NodeRole,
    site::{
        SecretRef, SiteConfig,
        network::{Network, Purpose},
        node::{ManagedPackage, NodeConfig},
    },
};
use uniwrt_uci::Package;

mod adoption;
mod backhaul;
mod blocking;
mod ddns;
mod dhcp;
mod doh;
mod ext;
mod failover;
mod firewall;
mod mdns;
mod mesh;
mod network;
mod peer_conf;
mod plan;
mod ports;
mod preserve;
pub mod raw;
mod redirect;
mod roaming;
mod split;
mod sqm;
mod staging;
mod switch;
mod system;
mod tailscale;
mod upnp;
mod usteer;
mod wan;
mod wireguard;
mod wireless;

/// What the gateway's VPNs need, where the packages they ride on are rendered: WireGuard's tools
/// with `network`, Tailscale's daemon with `firewall`.
fn vpn_requires(
    plan: &plan::Plan<'_>,
    packages: &BTreeMap<ManagedPackage, Package>,
) -> Vec<Requirement> {
    let mut requires = Vec::new();
    if plan.vpn.is_some() && packages.contains_key(&ManagedPackage::Network) {
        requires.push(wireguard::REQUIRES);
    }
    if plan.tailscale.is_some() && packages.contains_key(&ManagedPackage::Firewall) {
        requires.push(tailscale::REQUIRES);
    }
    requires
}

/// The vault's values of the site's secrets.
pub trait Secrets {
    fn get(&self, secret: &SecretRef) -> Option<&str>;

    /// The 802.11r key of the WLAN `wlan` (64 hex digits), which its access points protect
    /// the keys they send each other with under SAE: the controller derives it from a key of
    /// its own (D-085), so clients that know the passphrase can't.
    fn ft_key(&self, wlan: &str) -> Option<&str> {
        let _ = wlan;
        None
    }
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
    /// The controller's address as the gateway sees it: inside a network's subnet, the
    /// gateway's dnsmasq resolves `uniwrt.<domain>` to it.
    pub controller: Option<IpAddr>,
    /// The cables to other nodes that batman-adv runs over, as the apply stages them
    /// (features/mesh.md, Wired backhaul); empty for none.
    pub backhaul: &'a [Backhaul],
}

/// The controller's address that the node's `uniwrt` record holds (in `current`'s `dhcp`):
/// where the controller was when it last applied. A render compared with what the node holds,
/// rather than applied, passes it as [`Target::controller`].
pub fn held_controller(current: &BTreeMap<String, Package>) -> Option<IpAddr> {
    current
        .get("dhcp")?
        .sections_of("domain")
        .find(|d| d.option("name") == Some(uniwrt_model::site::gateway::CONTROLLER_HOST))?
        .option("ip")?
        .parse()
        .ok()
}

/// What a node is given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    /// The managed packages (D-011); the node's other packages are touched only by its raw
    /// sections (`patched`).
    pub packages: BTreeMap<ManagedPackage, Package>,
    /// The packages render doesn't write that the node's raw sections patch, by name: the
    /// node's own file ([`Target::current`]) with them applied (D-143). Drift doesn't watch
    /// them: the rest of the file stays the node's.
    pub patched: BTreeMap<String, Package>,
    /// Init scripts to enable (`true`) or disable, such as `dnsmasq` on an AP.
    pub services: BTreeMap<&'static str, bool>,
    /// The packages from OpenWrt's feeds the configuration needs on the node (SQM's, usteer),
    /// which an apply installs where they are missing (D-076).
    pub requires: Vec<Requirement>,
    pub warnings: Vec<Issue>,
}

/// A package from OpenWrt's feeds that a rendered configuration needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Requirement {
    /// Its name for opkg and apk alike, such as `sqm-scripts`.
    pub package: &'static str,
    /// The init script it brings, which render may switch on: absent until it is installed.
    pub service: Option<&'static str>,
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

/// The network that carries an AP's own address (a DHCP client): the management network, else
/// the default one; APs fall back to `lan` without either.
pub fn management_network(site: &SiteConfig) -> Option<&Network> {
    site.networks
        .iter()
        .find(|n| n.purpose == Purpose::Mgmt)
        .or_else(|| site.networks.iter().find(|n| n.purpose == Purpose::Default))
}

/// Whether a rendered section carries a node's uplink, the WAN (`wan`, `wan6`, `wan_6`) or the
/// mesh (`bat0`, `uw_mesh`, a wired backhaul's `uw_bh_<port>`): changing one takes longer to settle (a new lease, PPPoE, the mesh
/// meeting its neighbors again), so its transaction gets a longer timer.
pub fn uplink_section(name: &str) -> bool {
    name.starts_with("wan")
        || name == plan::BAT
        || name == mesh::MESH_IFACE
        || name.starts_with(BACKHAUL_PREFIX)
}

pub use self::{
    adoption::{ADOPT_BRIDGE, ADOPT_INTERFACE},
    backhaul::{
        BACKHAUL_PREFIX, Backhaul, BackhaulStage, backhaul_interface, backhaul_ports,
        held_backhauls, meshes_over_radio,
    },
    blocking::BYPASS_PREFIX as DNS_BYPASS_PREFIX,
    firewall::seeded,
    mesh::MESH_IFACE,
    peer_conf::{PeerKeys, peer_conf},
    plan::{BAT, BRIDGE},
    ports::port_names,
    split::{wan_interface, wan_sections, without_wan},
    staging::{Staging, staging},
    upnp::LEASE_FILE as UPNP_LEASE_FILE,
    usteer::thresholds as steering_thresholds,
};

/// The packages render can write; the other [`ManagedPackage`]s get renderers in later work.
pub const RENDERABLE: [ManagedPackage; 12] = [
    ManagedPackage::System,
    ManagedPackage::Network,
    ManagedPackage::Wireless,
    ManagedPackage::Dhcp,
    ManagedPackage::Firewall,
    ManagedPackage::Usteer,
    ManagedPackage::Sqm,
    ManagedPackage::Mwan3,
    ManagedPackage::Upnpd,
    ManagedPackage::HttpsDnsProxy,
    ManagedPackage::Ddns,
    ManagedPackage::MdnsRepeater,
];

/// A package of what the feeds bring, on the gateway: the package, its init script, whether
/// that runs, and the feed packages it needs then.
struct FeedRendered {
    package: Package,
    service: &'static str,
    on: bool,
    requires: Vec<Requirement>,
}

/// SQM's, mwan3's, miniupnpd's, https-dns-proxy's, ddns-scripts' or mdns-repeater's package; `None` where there is nothing to own (not the
/// gateway, or the feature off and the package not on the node).
fn feed_package(
    package: ManagedPackage,
    plan: &mut plan::Plan<'_>,
    held: bool,
) -> Option<FeedRendered> {
    let (rendered, service, on, requires) = match package {
        ManagedPackage::Sqm => {
            let on = plan.site.wan.sqm.is_some();
            (sqm::render(plan, held)?, "sqm", on, sqm::REQUIRES.to_vec())
        }
        ManagedPackage::Mwan3 => {
            let usb = matches!(
                plan.site.wan.failover.as_ref().map(|f| &f.link),
                Some(uniwrt_model::site::gateway::BackupLink::Usb { .. })
            );
            let mut requires = vec![failover::REQUIRES];
            if usb {
                requires.extend(failover::USB_REQUIRES);
            }
            let on = plan.backup.is_some();
            (failover::render(plan, held)?, "mwan3", on, requires)
        }
        ManagedPackage::Upnpd => {
            let on = plan.site.firewall.upnp.enabled;
            (
                upnp::render(plan, held)?,
                "miniupnpd",
                on,
                vec![upnp::REQUIRES],
            )
        }
        ManagedPackage::HttpsDnsProxy => {
            let on = doh::resolvers(plan).is_some();
            (
                doh::render(plan, held)?,
                "https-dns-proxy",
                on,
                vec![doh::REQUIRES],
            )
        }
        ManagedPackage::Ddns => {
            let rendered = ddns::render(plan, held)?;
            let on = ddns::enabled(plan).next().is_some();
            (rendered, "ddns", on, ddns::requires(plan))
        }
        ManagedPackage::MdnsRepeater => {
            let on = !mdns::networks(plan).is_empty();
            (
                mdns::render(plan, held)?,
                "mdns-repeater",
                on,
                vec![mdns::REQUIRES],
            )
        }
        _ => return None,
    };
    Some(FeedRendered {
        package: rendered,
        service,
        on,
        requires,
    })
}

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
    let mut requires = Vec::new();
    for package in config.managed.iter().filter(|p| RENDERABLE.contains(p)) {
        let held = target.current.contains_key(package.name());
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
            ManagedPackage::Usteer => {
                let Some((rendered, on)) = usteer::render(&mut plan, held) else {
                    continue;
                };
                services.insert("usteer", on);
                if on {
                    requires.push(usteer::REQUIRES);
                }
                rendered
            }
            ManagedPackage::Sqm
            | ManagedPackage::Mwan3
            | ManagedPackage::Upnpd
            | ManagedPackage::HttpsDnsProxy
            | ManagedPackage::Ddns
            | ManagedPackage::MdnsRepeater => {
                let Some(rendered) = feed_package(*package, &mut plan, held) else {
                    continue;
                };
                services.insert(rendered.service, rendered.on);
                if rendered.on {
                    requires.extend(rendered.requires);
                }
                rendered.package
            }
            ManagedPackage::Dropbear => continue,
        };
        let mut rendered = rendered;
        preserve::carry_over(&mut rendered, target.current);
        let at = raw::Merge::new(secrets, &mut plan.warnings, &mut plan.errors, false);
        raw::merge(&mut rendered, &config.raw, at);
        if let Err(invalid) = rendered.validate() {
            errors.push(Issue::new(
                "node",
                format!("The {} package came out invalid: {invalid}", rendered.name),
            ));
        }
        packages.insert(*package, rendered);
    }
    requires.extend(vpn_requires(&plan, &packages));
    let patched = patch(config, &packages, secrets, target, &mut plan, &mut errors);
    errors.append(&mut plan.errors);
    if errors.is_empty() {
        Ok(Rendered {
            packages,
            patched,
            services,
            requires,
            warnings: plan.warnings,
        })
    } else {
        Err(errors)
    }
}

/// The packages render didn't write that the node's raw sections patch: each the node's own
/// file with them applied (D-143). One the node lacks is left out with a warning.
fn patch(
    config: &NodeConfig,
    rendered: &BTreeMap<ManagedPackage, Package>,
    secrets: &dyn Secrets,
    target: Target<'_>,
    plan: &mut plan::Plan<'_>,
    errors: &mut Vec<Issue>,
) -> BTreeMap<String, Package> {
    let mut patched = BTreeMap::new();
    for (i, raw) in config.raw.iter().enumerate() {
        let name = raw.package.as_str();
        if patched.contains_key(name)
            || rendered.keys().any(|package| package.name() == name)
            || config.raw[..i].iter().any(|before| before.package == name)
        {
            continue;
        }
        let Some(held) = target.current.get(name) else {
            plan.warnings.push(Issue::new(
                format!("node/raw/{i}"),
                format!("The node has no /etc/config/{name}: its raw sections are left out"),
            ));
            continue;
        };
        let mut package = held.clone();
        let at = raw::Merge::new(secrets, &mut plan.warnings, &mut plan.errors, true);
        raw::merge(&mut package, &config.raw, at);
        if let Err(invalid) = package.validate() {
            errors.push(Issue::new(
                format!("node/raw/{i}"),
                format!("The {name} package came out invalid: {invalid}"),
            ));
        }
        patched.insert(name.to_owned(), package);
    }
    patched
}
