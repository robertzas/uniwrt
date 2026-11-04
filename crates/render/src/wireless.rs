//! The `wireless` package: a `wifi-device` per radio (its channel, width and power from the
//! band's policy and the node's overrides, within what its phy supports), each WLAN's
//! `wifi-iface` on every radio of its bands (`uw_<wlan>_<band>`, features/wifi.md), and the
//! mesh point.

use uniwrt_model::{
    facts::{PhyBand, RadioFacts, WifiFeature},
    site::{
        node::RadioOverride,
        wireless::{Channel, NodeScope, TxPower, Wlan, WlanSecurity},
    },
    wifi::{Band, ChannelWidth, WifiStandard},
};
use uniwrt_uci::{Package, Section};

use crate::{
    Issue,
    ext::SectionExt,
    mesh,
    plan::{Plan, band_name, mesh_default_channel},
};

pub(crate) fn render(plan: &mut Plan<'_>) -> Package {
    let mut radios: Vec<&RadioFacts> = plan.facts.radios.iter().collect();
    radios.sort_by(|a, b| a.name.cmp(&b.name));
    let wlans: Vec<&Wlan> = plan
        .site
        .wlans
        .iter()
        .filter(|wlan| in_scope(&wlan.nodes, plan))
        .collect();
    let mut devices = Vec::new();
    let mut ifaces = Vec::new();
    // How many radios of each band already carry WLANs, which numbers the next one's sections.
    let mut per_band: Vec<(Band, usize)> = Vec::new();
    for radio in radios {
        let tweak = plan.config.radios.iter().find(|r| r.radio == radio.name);
        let off = tweak.is_some_and(|t| t.disabled);
        let mesh_here = plan
            .mesh
            .as_ref()
            .is_some_and(|m| m.radio.name == radio.name);
        let dedicated = mesh_here && plan.mesh.as_ref().is_some_and(|m| m.dedicated);
        let mut carried = 0;
        if !off && !dedicated {
            let mine: Vec<&Wlan> = wlans
                .iter()
                .copied()
                .filter(|w| w.bands.contains(&radio.band))
                .collect();
            if !mine.is_empty() {
                let nth = if let Some((_, n)) = per_band.iter_mut().find(|(b, _)| *b == radio.band)
                {
                    *n += 1;
                    *n
                } else {
                    per_band.push((radio.band, 1));
                    1
                };
                for wlan in mine {
                    if let Some(iface) = ap_iface(plan, wlan, radio, nth) {
                        ifaces.push(iface);
                        carried += 1;
                    }
                }
            }
        }
        if mesh_here && !off {
            ifaces.extend(mesh::wifi_iface(plan));
            carried += 1;
        }
        if let Some(device) = device(plan, radio, tweak, carried == 0 || off) {
            devices.push(device);
        }
    }
    let mut package = Package::new("wireless");
    package.sections = devices;
    package.sections.append(&mut ifaces);
    package
}

fn in_scope(scope: &NodeScope, plan: &Plan<'_>) -> bool {
    let node = plan.config.node;
    match scope {
        NodeScope::All => true,
        NodeScope::Only(nodes) => nodes.contains(&node),
        NodeScope::Except(nodes) => !nodes.contains(&node),
    }
}

const fn band_key(band: Band) -> &'static str {
    match band {
        Band::G2 => "2g",
        Band::G5 => "5g",
        Band::G6 => "6g",
        Band::G60 => "60g",
    }
}

fn phy_band<'p>(plan: &Plan<'p>, radio: &RadioFacts) -> Option<&'p PhyBand> {
    plan.facts
        .phys
        .iter()
        .find(|phy| Some(&phy.name) == radio.phy.as_ref())
        .and_then(|phy| phy.bands.iter().find(|b| b.band == radio.band))
}

fn device(
    plan: &mut Plan<'_>,
    radio: &RadioFacts,
    tweak: Option<&RadioOverride>,
    idle: bool,
) -> Option<Section> {
    let at = format!("node/radios/{}", radio.name);
    let section = Section::named("wifi-device", radio.name.as_str()).with("type", "mac80211");
    let section = match (&radio.path, &radio.phy) {
        (Some(path), _) => section.with("path", path.as_str()),
        (None, Some(phy)) => section.with("phy", phy.as_str()),
        (None, None) => {
            plan.errors.push(Issue::new(
                at,
                "The radio has neither a path nor a phy to bind it to its hardware",
            ));
            return None;
        }
    };
    let policy = plan.site.radio.bands.iter().find(|p| p.band == radio.band);
    let phy = phy_band(plan, radio);
    let channel = channel(plan, radio, tweak, phy, &at);
    let width = tweak
        .and_then(|t| t.width)
        .or(policy.map(|p| p.width))
        .unwrap_or(ChannelWidth::Mhz20);
    let htmode = htmode(radio, phy, width);
    let power = tweak
        .and_then(|t| t.tx_power)
        .or(policy.map(|p| p.tx_power));
    Some(
        section
            .with("band", band_key(radio.band))
            .with("channel", channel)
            .maybe("htmode", htmode)
            .maybe("country", plan.site.site.country.as_deref())
            .maybe(
                "txpower",
                match power {
                    Some(TxPower::Dbm(dbm)) => Some(dbm.to_string()),
                    Some(TxPower::Auto) | None => None,
                },
            )
            .flag("disabled", idle),
    )
}

fn channel(
    plan: &mut Plan<'_>,
    radio: &RadioFacts,
    tweak: Option<&RadioOverride>,
    phy: Option<&PhyBand>,
    at: &str,
) -> String {
    let wanted = tweak.and_then(|t| t.channel);
    if let Some(mesh) = plan.mesh.as_ref().filter(|m| m.radio.name == radio.name) {
        let channel = mesh.channel;
        if wanted.is_some_and(|w| w != Channel::Fixed(channel)) {
            plan.warnings.push(Issue::new(
                at,
                format!("The radio carries the mesh, which keeps it on channel {channel}"),
            ));
        }
        return channel.to_string();
    }
    // A tri-band node's other radio of the mesh's band keeps off the mesh's channel.
    let mesh_channel = plan
        .mesh
        .as_ref()
        .filter(|m| m.dedicated && m.radio.band == radio.band)
        .map(|m| m.channel);
    let policy = plan
        .site
        .radio
        .bands
        .iter()
        .find(|p| p.band == radio.band)
        .map(|p| match p.channel {
            Channel::Fixed(n) if Some(n) == mesh_channel => Channel::Auto,
            channel => channel,
        });
    match wanted.or(policy).unwrap_or(Channel::Auto) {
        Channel::Fixed(n) => {
            if phy.is_some_and(|phy| !phy.channels.iter().any(|c| c.number == n)) {
                plan.errors
                    .push(Issue::new(at, format!("The radio can't use channel {n}")));
            }
            n.to_string()
        }
        Channel::Auto if plan.facts.capabilities.wifi.contains(&WifiFeature::Acs) => {
            "auto".to_owned()
        }
        Channel::Auto => {
            // Without ACS hostapd can't choose: keep the radio where it is, or on a channel
            // that needs no radar detection.
            let channel = radio
                .channel
                .filter(|c| Some(*c) != mesh_channel)
                .or_else(|| {
                    phy.and_then(|phy| {
                        phy.channels
                            .iter()
                            .find(|c| !c.dfs && !c.no_ir && Some(c.number) != mesh_channel)
                            .map(|c| c.number)
                    })
                })
                .unwrap_or(mesh_default_channel(radio.band));
            plan.warnings.push(Issue::new(
                at,
                format!(
                    "The node can't choose {} channels itself (its hostapd has no ACS): the radio stays on channel {channel}",
                    band_name(radio.band)
                ),
            ));
            channel.to_string()
        }
    }
}

/// UCI `htmode`: the phy's newest standard at the widest width it supports up to `width`.
fn htmode(radio: &RadioFacts, phy: Option<&PhyBand>, width: ChannelWidth) -> Option<String> {
    let (standard, widths): (WifiStandard, Vec<ChannelWidth>) = match phy {
        Some(phy) => (
            phy.standards.iter().max().copied()?,
            phy.widths.iter().copied().collect(),
        ),
        // A radio that is down tells no phy: keep its configured standard.
        None => (
            radio.standard?,
            vec![radio.width.unwrap_or(ChannelWidth::Mhz20)],
        ),
    };
    let cap = match standard {
        WifiStandard::Legacy => return Some("NOHT".to_owned()),
        WifiStandard::Ht => ChannelWidth::Mhz40,
        WifiStandard::Vht | WifiStandard::He => ChannelWidth::Mhz160,
        WifiStandard::Eht => ChannelWidth::Mhz320,
    };
    let width = widths
        .into_iter()
        .filter(|w| *w <= width && *w <= cap)
        .max()
        .unwrap_or(ChannelWidth::Mhz20);
    let prefix = match standard {
        WifiStandard::Ht => "HT",
        WifiStandard::Vht => "VHT",
        WifiStandard::He => "HE",
        WifiStandard::Eht | WifiStandard::Legacy => "EHT",
    };
    Some(format!("{prefix}{}", width.mhz()))
}

/// A WLAN's BSS on one radio; `nth` numbers the radios of a band that carry WLANs.
fn ap_iface(plan: &mut Plan<'_>, wlan: &Wlan, radio: &RadioFacts, nth: usize) -> Option<Section> {
    let at = format!("wlans/{}", wlan.id);
    let network = plan.net(&wlan.network)?.network;
    let wifi = &plan.facts.capabilities.wifi;
    let (encryption, pmf) = match wlan.security {
        WlanSecurity::Wpa2 => ("psk2", None),
        WlanSecurity::Wpa2Wpa3 if wifi.contains(&WifiFeature::Sae) => ("sae-mixed", Some("1")),
        WlanSecurity::Wpa2Wpa3 => {
            plan.warnings.push(Issue::new(
                &at,
                "The node's hostapd has no WPA3 (SAE): it offers WPA2 only",
            ));
            ("psk2", None)
        }
        WlanSecurity::Wpa3 if wifi.contains(&WifiFeature::Sae) => ("sae", Some("2")),
        WlanSecurity::Owe if wifi.contains(&WifiFeature::Owe) => ("owe", Some("2")),
        WlanSecurity::Wpa3 | WlanSecurity::Owe => {
            plan.errors.push(Issue::new(
                &at,
                "The node's hostapd lacks this security mode: install the full wpad",
            ));
            return None;
        }
        WlanSecurity::Open => ("none", None),
    };
    let key = if wlan.security.needs_passphrase() {
        Some(plan.secret(&format!("{at}/passphrase"), wlan.passphrase.as_ref())?)
    } else {
        None
    };
    let suffix = if nth > 1 {
        nth.to_string()
    } else {
        String::new()
    };
    let name = format!("uw_{}_{}{suffix}", wlan.id, band_key(radio.band));
    Some(
        Section::named("wifi-iface", name)
            .with("device", radio.name.as_str())
            .with("network", network.id.as_str())
            .with("mode", "ap")
            .with("ssid", wlan.ssid.as_str())
            .with("encryption", encryption)
            .maybe("key", key)
            .maybe("ieee80211w", pmf)
            .flag("hidden", wlan.hidden)
            .flag("isolate", wlan.isolation || network.isolation)
            .flag("disabled", !wlan.enabled),
    )
}
