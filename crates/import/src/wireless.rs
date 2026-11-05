//! Wi-Fi, from every node (features/wifi.md, features/mesh.md): the access points of the same
//! SSID, network and security become one WLAN on the bands and nodes that carry it; a mesh
//! point with batman-adv over it becomes the mesh; each band's most common channel, width and
//! power become its policy, and the radios that differ get overrides; the radios' country
//! becomes the site's.

use std::collections::{BTreeMap, BTreeSet};

use uniwrt_core::NodeId;
use uniwrt_model::{
    site::{
        SecretRef, SiteConfig,
        node::RadioOverride,
        wireless::{
            BandPolicy, Channel, MeshConfig, MeshRouting, NodeScope, TxPower, Wlan, WlanSecurity,
        },
    },
    wifi::{Band, ChannelWidth},
};
use uniwrt_uci::Section;

use crate::{Source, node_config, read};

/// A radio as a node configures it.
struct Radio {
    node: NodeId,
    name: String,
    band: Band,
    channel: Channel,
    width: Option<ChannelWidth>,
    power: TxPower,
    disabled: bool,
}

/// The access points of one WLAN, the bands and nodes that carry them.
type Group = (Ap, BTreeSet<Band>, BTreeSet<NodeId>);

/// What one access point broadcasts, as a WLAN would hold it.
#[derive(Clone, PartialEq, Eq)]
struct Ap {
    ssid: String,
    network: String,
    /// The index of its [`WlanSecurity`] in [`SECURITIES`] (which isn't ordered).
    security: usize,
    key: Option<String>,
    hidden: bool,
    isolation: bool,
    enabled: bool,
}

const SECURITIES: [WlanSecurity; 5] = [
    WlanSecurity::Wpa2,
    WlanSecurity::Wpa2Wpa3,
    WlanSecurity::Wpa3,
    WlanSecurity::Owe,
    WlanSecurity::Open,
];

pub(crate) fn import(
    site: &mut SiteConfig,
    sources: &[Source<'_>],
    renames: &read::Renames,
    seal: &mut dyn FnMut(&str) -> SecretRef,
) {
    if !sources.iter().any(|s| s.packages.contains_key("wireless")) {
        return;
    }
    let networks: BTreeSet<String> = site.networks.iter().map(|n| n.id.clone()).collect();
    let mut radios = Vec::new();
    let mut aps: Vec<Group> = Vec::new();
    // The id a WLAN's section name gives (`uw_<id>_<band>`), as uniwrt named it.
    let mut hints: Vec<Option<String>> = Vec::new();
    // The WLANs' order: each radio lists its access points in the site's order, so a new one
    // goes right after the one its radio listed before it.
    let mut order: Vec<usize> = Vec::new();
    let mut country = None;
    for source in sources {
        let mut previous: BTreeMap<String, usize> = BTreeMap::new();
        let devices: Vec<&Section> =
            read::sections(source.packages, "wireless", "wifi-device").collect();
        let band_of = |device: &str| -> Option<Band> {
            source
                .facts
                .radios
                .iter()
                .find(|r| r.name == device)
                .map(|r| r.band)
                .or_else(|| {
                    let section = devices.iter().find(|d| d.name.as_deref() == Some(device))?;
                    serde_band(section.option("band")?)
                })
        };
        for device in &devices {
            let Some(name) = device.name.as_deref() else {
                continue;
            };
            let Some(band) = band_of(name) else { continue };
            if country.is_none() {
                country = device.option("country").map(str::to_owned);
            }
            radios.push(Radio {
                node: source.node,
                name: name.to_owned(),
                band,
                channel: match device.option("channel") {
                    Some(n) if n != "auto" => n.parse().map_or(Channel::Auto, Channel::Fixed),
                    _ => Channel::Auto,
                },
                width: device.option("htmode").and_then(width),
                power: device
                    .option("txpower")
                    .and_then(|p| p.parse().ok())
                    .map_or(TxPower::Auto, TxPower::Dbm),
                disabled: read::flag(device, "disabled") == Some(true),
            });
        }
        for iface in read::sections(source.packages, "wireless", "wifi-iface") {
            if !matches!(iface.option("mode"), None | Some("ap")) {
                continue;
            }
            // A disabled radio broadcasts nothing (OpenWrt ships its radios so, with an open
            // `OpenWrt`): its access points are kept as they are, raw.
            let device = iface.option("device").unwrap_or_default();
            let off = radios
                .iter()
                .any(|r| r.node == source.node && r.name == device && r.disabled);
            let Some(band) = band_of(device).filter(|_| !off) else {
                continue;
            };
            let Some(ap) = ap(iface, renames, &networks) else {
                continue;
            };
            let index = aps.iter().position(|(a, ..)| *a == ap).unwrap_or_else(|| {
                aps.push((ap, BTreeSet::new(), BTreeSet::new()));
                hints.push(iface.name.as_deref().and_then(hint));
                let index = aps.len() - 1;
                let at = previous
                    .get(device)
                    .and_then(|p| order.iter().position(|o| o == p))
                    .map_or(order.len(), |p| p + 1);
                order.insert(at, index);
                index
            });
            previous.insert(device.to_owned(), index);
            aps[index].1.insert(band);
            aps[index].2.insert(source.node);
        }
    }
    let mut aps: Vec<Option<_>> = aps.into_iter().zip(hints).map(Some).collect();
    let aps = order.iter().filter_map(|&i| aps[i].take()).collect();
    site.wlans = wlans(site, sources, &radios, aps, seal);
    if let Some(mesh) = mesh(sources, seal) {
        site.mesh = mesh;
    }
    if country.is_some() {
        site.site.country = country;
    }
    radio_policy(site, &radios);
}

fn ap(iface: &Section, renames: &read::Renames, networks: &BTreeSet<String>) -> Option<Ap> {
    let ssid = iface.option("ssid")?.to_owned();
    let network = renames
        .network(read::items(iface, "network").first()?)
        .to_owned();
    if !networks.contains(&network) {
        return None;
    }
    let encryption = iface.option("encryption").unwrap_or("none");
    let security = match encryption.split('+').next().unwrap_or_default() {
        "psk2" | "psk" | "psk-mixed" => WlanSecurity::Wpa2,
        "sae-mixed" => WlanSecurity::Wpa2Wpa3,
        "sae" => WlanSecurity::Wpa3,
        "owe" => WlanSecurity::Owe,
        "none" | "" => WlanSecurity::Open,
        _ => return None,
    };
    let key = iface.option("key").map(str::to_owned);
    if security.needs_passphrase() && key.is_none() {
        return None;
    }
    Some(Ap {
        ssid,
        network,
        security: SECURITIES.iter().position(|s| *s == security)?,
        key: key.filter(|_| security.needs_passphrase()),
        hidden: read::flag(iface, "hidden").unwrap_or(false),
        isolation: read::flag(iface, "isolate").unwrap_or(false),
        enabled: read::flag(iface, "disabled") != Some(true),
    })
}

fn wlans(
    site: &SiteConfig,
    sources: &[Source<'_>],
    radios: &[Radio],
    aps: Vec<(Group, Option<String>)>,
    seal: &mut dyn FnMut(&str) -> SecretRef,
) -> Vec<Wlan> {
    let mut found: Vec<Wlan> = Vec::new();
    for ((ap, bands, nodes), hint) in aps {
        let taken = |id: &str| found.iter().any(|w| w.id == id);
        let id = site
            .wlans
            .iter()
            .find(|w| w.ssid == ap.ssid && !taken(&w.id))
            .map(|w| w.id.clone())
            .or(hint.filter(|h| !taken(h)))
            .unwrap_or_else(|| read::id(&ap.ssid, "w", taken));
        let candidates: BTreeSet<NodeId> = sources
            .iter()
            .filter(|s| {
                radios
                    .iter()
                    .any(|r| r.node == s.node && bands.contains(&r.band))
            })
            .map(|s| s.node)
            .collect();
        let scope = if nodes.is_superset(&candidates) {
            NodeScope::All
        } else {
            NodeScope::Only(nodes.into_iter().collect())
        };
        found.push(Wlan {
            id,
            ssid: ap.ssid,
            enabled: ap.enabled,
            network: ap.network,
            security: SECURITIES[ap.security],
            passphrase: ap.key.as_deref().map(&mut *seal),
            bands: bands.into_iter().collect(),
            nodes: scope,
            hidden: ap.hidden,
            isolation: ap.isolation,
        });
    }
    found
}

/// The mesh: an 802.11s point whose network is a batman-adv hard interface.
fn mesh(sources: &[Source<'_>], seal: &mut dyn FnMut(&str) -> SecretRef) -> Option<MeshConfig> {
    for source in sources {
        let network = source.packages.get("network");
        for iface in read::sections(source.packages, "wireless", "wifi-iface") {
            if iface.option("mode") != Some("mesh") {
                continue;
            }
            let hardif = read::items(iface, "network")
                .first()
                .and_then(|n| network?.section(n))
                .filter(|s| s.option("proto") == Some("batadv_hardif"));
            let batman = hardif
                .and_then(|h| h.option("master"))
                .and_then(|m| network?.section(m))
                .filter(|s| s.option("proto") == Some("batadv"));
            let (Some(batman), Some(mesh_id), Some(key)) =
                (batman, iface.option("mesh_id"), iface.option("key"))
            else {
                continue;
            };
            let band = iface.option("device").and_then(|device| {
                source
                    .facts
                    .radios
                    .iter()
                    .find(|r| r.name == device)
                    .map(|r| r.band)
            });
            return Some(MeshConfig {
                enabled: true,
                mesh_id: mesh_id.to_owned(),
                key: Some(seal(key)),
                band,
                routing: match batman.option("routing_algo") {
                    Some("BATMAN_V") => MeshRouting::BatmanV,
                    _ => MeshRouting::BatmanIv,
                },
            });
        }
    }
    None
}

/// Each band's most common setting becomes its policy; radios that differ get overrides.
fn radio_policy(site: &mut SiteConfig, radios: &[Radio]) {
    let mut overrides: BTreeMap<NodeId, Vec<RadioOverride>> = BTreeMap::new();
    for band in [Band::G2, Band::G5, Band::G6, Band::G60] {
        let of_band: Vec<&Radio> = radios.iter().filter(|r| r.band == band).collect();
        if of_band.is_empty() {
            continue;
        }
        let on = || of_band.iter().filter(|r| !r.disabled);
        let channel = most_common(on().map(|r| r.channel));
        let width = most_common(on().filter_map(|r| r.width));
        let power = most_common(on().map(|r| r.power));
        if !site.radio.bands.iter().any(|p| p.band == band) {
            site.radio.bands.push(BandPolicy {
                band,
                channel: Channel::Auto,
                width: ChannelWidth::Mhz20,
                tx_power: TxPower::Auto,
            });
        }
        let index = site
            .radio
            .bands
            .iter()
            .position(|p| p.band == band)
            .unwrap_or_default();
        let policy = &mut site.radio.bands[index];
        if let Some(channel) = channel {
            policy.channel = channel;
        }
        if let Some(width) = width {
            policy.width = width;
        }
        if let Some(power) = power {
            policy.tx_power = power;
        }
        let policy = policy.clone();
        for radio in of_band {
            let tweak = RadioOverride {
                radio: radio.name.clone(),
                disabled: radio.disabled,
                channel: (radio.channel != policy.channel).then_some(radio.channel),
                width: radio.width.filter(|w| *w != policy.width),
                tx_power: (radio.power != policy.tx_power).then_some(radio.power),
            };
            if tweak.disabled
                || tweak.channel.is_some()
                || tweak.width.is_some()
                || tweak.tx_power.is_some()
            {
                overrides.entry(radio.node).or_default().push(tweak);
            }
        }
    }
    let nodes: BTreeSet<NodeId> = radios.iter().map(|r| r.node).collect();
    for node in nodes {
        node_config(site, node).radios = overrides.remove(&node).unwrap_or_default();
    }
}

/// The WLAN id in a section name uniwrt gave (`uw_<id>_<band>[n]`).
fn hint(name: &str) -> Option<String> {
    let rest = name
        .strip_prefix("uw_")?
        .trim_end_matches(|c: char| c.is_ascii_digit());
    let id = ["_2g", "_5g", "_6g", "_60g"]
        .iter()
        .find_map(|band| rest.strip_suffix(band))?;
    uniwrt_model::site::validate::is_id(id).then(|| id.to_owned())
}

/// The value most items share; the first among equals.
fn most_common<T: Copy + PartialEq>(items: impl Iterator<Item = T>) -> Option<T> {
    let mut counts: Vec<(T, usize)> = Vec::new();
    for item in items {
        match counts.iter_mut().find(|(v, _)| *v == item) {
            Some((_, n)) => *n += 1,
            None => counts.push((item, 1)),
        }
    }
    let best = counts.iter().map(|(_, n)| *n).max()?;
    counts.into_iter().find(|(_, n)| *n == best).map(|(v, _)| v)
}

/// A channel width from `htmode` (`HT40+`, `VHT80`, `HE160`, `EHT320`; `NOHT` is 20 MHz).
fn width(htmode: &str) -> Option<ChannelWidth> {
    if htmode == "NOHT" {
        return Some(ChannelWidth::Mhz20);
    }
    let digits: String = htmode
        .trim_start_matches(|c: char| c.is_ascii_alphabetic())
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    ChannelWidth::try_from(digits.parse::<u16>().ok()?).ok()
}

fn serde_band(band: &str) -> Option<Band> {
    match band {
        "2g" => Some(Band::G2),
        "5g" => Some(Band::G5),
        "6g" => Some(Band::G6),
        "60g" => Some(Band::G60),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use uniwrt_model::wifi::ChannelWidth;

    use super::{hint, most_common, width};

    #[test]
    fn uniwrt_s_section_names_give_their_wlan() {
        assert_eq!(hint("uw_guest_2g").as_deref(), Some("guest"));
        assert_eq!(hint("uw_home_net_5g2").as_deref(), Some("home_net"));
        assert_eq!(hint("default_radio0"), None);
        assert_eq!(hint("uw_mesh"), None);
    }

    #[test]
    fn htmodes_read_as_widths() {
        assert_eq!(width("HT40+"), Some(ChannelWidth::Mhz40));
        assert_eq!(width("HE80"), Some(ChannelWidth::Mhz80));
        assert_eq!(width("EHT320"), Some(ChannelWidth::Mhz320));
        assert_eq!(width("NOHT"), Some(ChannelWidth::Mhz20));
        assert_eq!(width("HE"), None);
    }

    #[test]
    fn the_most_common_value_wins_and_ties_go_first() {
        assert_eq!(most_common([1, 2, 2, 3].into_iter()), Some(2));
        assert_eq!(most_common([3, 1].into_iter()), Some(3));
        assert_eq!(most_common(std::iter::empty::<u8>()), None);
    }
}
