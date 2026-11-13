//! The `wireless` package: a `wifi-device` per radio (its channel, width and power from the
//! node's overrides and the band's policy, and where those leave it on Auto from the RF plan the
//! site follows, D-112; within what its phy supports), each WLAN's
//! `wifi-iface` on every radio of its bands (`uw_<wlan>_<band>`, features/wifi.md), and the
//! mesh point.

use uniwrt_model::{
    facts::{PhyBand, RadioFacts, WifiFeature},
    site::{
        node::RadioOverride,
        wireless::{Channel, PlannedRadio, TxPower, Wlan, WlanSecurity},
    },
    wifi::{Band, ChannelWidth, WifiStandard},
};
use uniwrt_uci::{Package, Section};

use crate::{
    Issue,
    ext::SectionExt,
    mesh,
    plan::{Plan, band_name, mesh_default_channel},
    roaming,
};

pub(crate) fn render(plan: &mut Plan<'_>) -> Package {
    let mut radios: Vec<&RadioFacts> = plan.facts.radios.iter().collect();
    radios.sort_by(|a, b| a.name.cmp(&b.name));
    let wlans: Vec<&Wlan> = plan
        .site
        .wlans
        .iter()
        .filter(|wlan| wlan.nodes.holds(plan.config.node))
        .collect();
    let mut devices = Vec::new();
    let mut ifaces = Vec::new();
    // How many radios of each band already carry WLANs, which numbers the next one's sections.
    let mut per_band: Vec<(Band, usize)> = Vec::new();
    // How many OWE transition pairs the node has, which names the next one's interfaces.
    let mut pairs = 0;
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
                    let bss = ap_ifaces(plan, wlan, radio, nth, &mut pairs);
                    carried += bss.len();
                    ifaces.extend(bss);
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
    let planned = plan
        .site
        .radio
        .plan
        .radios
        .iter()
        .find(|p| p.node == plan.config.node && p.radio == radio.name && p.band == radio.band);
    let channel = channel(plan, radio, tweak, phy, planned, &at);
    // The plan's width goes with its channel: the mesh's on every node, else the radio's own.
    let mesh_width = plan
        .mesh
        .as_ref()
        .filter(|m| m.radio.name == radio.name)
        .and_then(|m| m.width);
    let width = tweak
        .and_then(|t| t.width)
        .or(mesh_width)
        .or(planned
            .filter(|p| channel == p.channel.to_string())
            .map(|p| p.width))
        .or(policy.map(|p| p.width))
        .unwrap_or(ChannelWidth::Mhz20);
    let htmode = htmode(radio, phy, width);
    let power = match tweak
        .and_then(|t| t.tx_power)
        .or(policy.map(|p| p.tx_power))
    {
        Some(TxPower::Dbm(dbm)) => Some(TxPower::Dbm(dbm)),
        Some(TxPower::Auto) | None => planned.map(|p| p.tx_power),
    };
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
    planned: Option<&PlannedRadio>,
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
    let chosen = wanted.or(policy).unwrap_or(Channel::Auto);
    // Left on Auto, the radio takes the RF plan's channel (D-112).
    if chosen == Channel::Auto
        && let Some(n) = planned
            .map(|p| p.channel)
            .filter(|n| Some(*n) != mesh_channel)
    {
        if phy.is_none_or(|phy| phy.channels.iter().any(|c| c.number == n)) {
            return n.to_string();
        }
        plan.warnings.push(Issue::new(
            at,
            format!("The radio can't use the RF plan's channel {n}: plan again"),
        ));
    }
    match chosen {
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

/// A WLAN's BSSes on one radio: one, or OWE's pair in transition mode (the open BSS under the
/// WLAN's name and the encrypted one hidden as `<ssid>OWE`, as OpenWrt 25.12 names it, each
/// pointing at the other's interface: `uw<n>-open` and `uw<n>-owe`, the node's `pairs`-th).
/// `nth` numbers the radios of a band that carry WLANs.
fn ap_ifaces(
    plan: &mut Plan<'_>,
    wlan: &Wlan,
    radio: &RadioFacts,
    nth: usize,
    pairs: &mut usize,
) -> Vec<Section> {
    let at = format!("wlans/{}", wlan.id);
    let Some(network) = plan.net(&wlan.network).map(|net| net.network) else {
        return Vec::new();
    };
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
        WlanSecurity::Owe | WlanSecurity::OweTransition if wifi.contains(&WifiFeature::Owe) => {
            ("owe", Some("2"))
        }
        WlanSecurity::Wpa3 | WlanSecurity::Owe | WlanSecurity::OweTransition => {
            plan.errors.push(Issue::new(
                &at,
                "The node's hostapd lacks this security mode: install the full wpad",
            ));
            return Vec::new();
        }
        WlanSecurity::Open => ("none", None),
    };
    let key = if wlan.security.needs_passphrase() {
        let Some(key) = plan.secret(&format!("{at}/passphrase"), wlan.passphrase.as_ref()) else {
            return Vec::new();
        };
        Some(key)
    } else {
        None
    };
    let suffix = if nth > 1 {
        nth.to_string()
    } else {
        String::new()
    };
    let name = format!("uw_{}_{}{suffix}", wlan.id, band_key(radio.band));
    let bss = |name: String, ssid: &str, encryption: &str| {
        Section::named("wifi-iface", name)
            .with("device", radio.name.as_str())
            .with("network", network.id.as_str())
            .with("mode", "ap")
            .with("ssid", ssid)
            .with("encryption", encryption)
    };
    let common = |section: Section| {
        section
            .maybe("maxassoc", wlan.max_clients.map(|max| max.to_string()))
            .flag("isolate", wlan.isolation || network.isolation)
            .flag("disabled", !wlan.enabled)
    };
    // 6 GHz has no open BSS, so the encrypted one stands alone there.
    if wlan.security == WlanSecurity::OweTransition && radio.band != Band::G6 {
        *pairs += 1;
        let (open, owe) = (format!("uw{pairs}-open"), format!("uw{pairs}-owe"));
        let open_bss = bss(format!("{name}o"), &wlan.ssid, "none")
            .with("ifname", open.as_str())
            .with("owe_transition_ifname", owe.as_str())
            .flag("hidden", wlan.hidden);
        let owe_bss = bss(name, &format!("{}OWE", wlan.ssid), encryption)
            .with("ifname", owe.as_str())
            .with("owe_transition_ifname", open.as_str())
            .maybe("ieee80211w", pmf)
            .flag("hidden", true);
        return vec![
            common(roaming::options(plan, wlan, "none", open_bss)),
            common(roaming::options(plan, wlan, encryption, owe_bss)),
        ];
    }
    let section = bss(name, &wlan.ssid, encryption)
        .maybe("key", key)
        .maybe("ieee80211w", pmf)
        .flag("hidden", wlan.hidden);
    vec![common(roaming::options(plan, wlan, encryption, section))]
}
