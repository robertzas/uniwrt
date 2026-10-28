//! The `radios` set: netifd's `network.wireless status` (each radio's configuration and state),
//! nl80211's interfaces (channel, width, power and addresses) and each phy's channel survey. A
//! radio's phy is the phy of its interfaces; while it has none, the one its `phy` option names or
//! whose device is its `path`.

use std::collections::{BTreeMap, HashMap};

use serde::Deserialize;
use uniwrt_model::{
    facts::{RadioFacts, WifiIfaceFacts, WifiIfaceMode},
    wifi::WifiStandard,
};

use super::{round, wifi};

#[derive(Deserialize)]
pub(super) struct Raw {
    wireless: BTreeMap<String, Radio>,
    #[serde(default)]
    phys: Vec<Phy>,
    #[serde(default)]
    interfaces: Vec<NlIface>,
    #[serde(default)]
    surveys: BTreeMap<String, Option<Survey>>,
}

#[derive(Deserialize)]
struct Radio {
    #[serde(default)]
    up: bool,
    #[serde(default)]
    disabled: bool,
    #[serde(default)]
    config: RadioConfig,
    #[serde(default)]
    interfaces: Vec<Iface>,
}

#[derive(Default, Deserialize)]
struct RadioConfig {
    band: Option<String>,
    htmode: Option<String>,
    country: Option<String>,
    path: Option<String>,
    phy: Option<String>,
}

#[derive(Deserialize)]
struct Iface {
    section: Option<String>,
    ifname: Option<String>,
    #[serde(default)]
    config: IfaceConfig,
}

#[derive(Default, Deserialize)]
struct IfaceConfig {
    mode: Option<String>,
    ssid: Option<String>,
    mesh_id: Option<String>,
    encryption: Option<String>,
    #[serde(default)]
    network: Vec<String>,
}

#[derive(Deserialize)]
struct Phy {
    name: String,
    index: u32,
    device: Option<String>,
}

#[derive(Deserialize)]
struct NlIface {
    ifname: String,
    wiphy: u32,
    mac: Option<String>,
    wiphy_freq: Option<u32>,
    channel_width: Option<u32>,
    center_freq1: Option<u32>,
    wiphy_tx_power_level: Option<u32>,
}

#[derive(Deserialize)]
struct Survey {
    noise: Option<i16>,
    busy_percent: Option<f64>,
}

pub(super) fn parse(raw: Raw) -> Vec<RadioFacts> {
    let nl: HashMap<&str, &NlIface> = raw
        .interfaces
        .iter()
        .map(|iface| (iface.ifname.as_str(), iface))
        .collect();
    raw.wireless
        .into_iter()
        .filter_map(|(name, radio)| {
            let ifaces: Vec<_> = radio
                .interfaces
                .iter()
                .map(|iface| {
                    (
                        iface,
                        iface.ifname.as_deref().and_then(|n| nl.get(n).copied()),
                    )
                })
                .collect();
            // The interface that tells the radio's channel: one that operates.
            let live = ifaces
                .iter()
                .find_map(|(_, nl)| nl.filter(|nl| nl.wiphy_freq.is_some()));
            let phy = ifaces
                .iter()
                .find_map(|(_, nl)| *nl)
                .and_then(|nl| raw.phys.iter().find(|phy| phy.index == nl.wiphy))
                .map(|phy| phy.name.clone())
                .or_else(|| radio.config.phy.clone())
                .or_else(|| by_path(&raw.phys, radio.config.path.as_deref()?));
            let frequency_mhz = live.and_then(|nl| nl.wiphy_freq);
            let band = radio
                .config
                .band
                .as_deref()
                .and_then(wifi::band_named)
                .or_else(|| wifi::band_of(frequency_mhz?))?;
            let survey = phy
                .as_ref()
                .and_then(|phy| raw.surveys.get(phy))
                .and_then(Option::as_ref);
            Some(RadioFacts {
                name,
                phy,
                band,
                up: radio.up,
                disabled: radio.disabled,
                channel: frequency_mhz.and_then(|mhz| wifi::channel(band, mhz)),
                frequency_mhz,
                center_frequency_mhz: live.and_then(|nl| nl.center_freq1),
                width: live.and_then(|nl| wifi::width(nl.channel_width?)),
                standard: radio.config.htmode.as_deref().and_then(standard),
                tx_power_dbm: live.and_then(|nl| wifi::dbm(nl.wiphy_tx_power_level?)),
                noise_dbm: survey.and_then(|survey| survey.noise),
                channel_busy_percent: survey
                    .and_then(|survey| survey.busy_percent)
                    .map(|percent| round(percent, 2)),
                // `00` is the world regulatory domain, not a country.
                country: radio
                    .config
                    .country
                    .clone()
                    .filter(|country| country != "00"),
                ifaces: ifaces
                    .into_iter()
                    .filter_map(|(iface, nl)| wifi_iface(iface, nl, radio.up))
                    .collect(),
            })
        })
        .collect()
}

/// The phy whose device is a radio's `path`. Phys that share a device (two radios on one card)
/// are told apart by a `+N` suffix, counting them in index order.
fn by_path(phys: &[Phy], path: &str) -> Option<String> {
    let (base, nth) = match path.rsplit_once('+') {
        Some((base, nth)) => (base, nth.parse().ok()?),
        None => (path, 0),
    };
    let mut matching: Vec<&Phy> = phys
        .iter()
        .filter(|phy| {
            let device = phy.device.as_deref().unwrap_or_default();
            device.strip_prefix("/sys/devices/") == Some(base)
        })
        .collect();
    matching.sort_by_key(|phy| phy.index);
    matching.get(nth).map(|phy| phy.name.clone())
}

/// The standard of a UCI `htmode`, such as `HE80` or `NOHT`.
fn standard(htmode: &str) -> Option<WifiStandard> {
    [
        ("NOHT", WifiStandard::Legacy),
        ("EHT", WifiStandard::Eht),
        ("HE", WifiStandard::He),
        ("VHT", WifiStandard::Vht),
        ("HT", WifiStandard::Ht),
    ]
    .into_iter()
    .find_map(|(prefix, standard)| htmode.starts_with(prefix).then_some(standard))
}

fn wifi_iface(iface: &Iface, nl: Option<&NlIface>, radio_up: bool) -> Option<WifiIfaceFacts> {
    let mode = match iface.config.mode.as_deref()? {
        "ap" => WifiIfaceMode::Ap,
        "sta" => WifiIfaceMode::Sta,
        "mesh" => WifiIfaceMode::Mesh,
        "adhoc" => WifiIfaceMode::Adhoc,
        "monitor" => WifiIfaceMode::Monitor,
        _ => return None,
    };
    let config = &iface.config;
    Some(WifiIfaceFacts {
        name: iface.ifname.clone(),
        section: iface.section.clone(),
        mode,
        ssid: config.ssid.clone(),
        mesh_id: config.mesh_id.clone(),
        // A station's BSS is its AP's, which the `stations` set reports.
        bssid: nl
            .filter(|_| mode != WifiIfaceMode::Sta && mode != WifiIfaceMode::Monitor)
            .and_then(|nl| nl.mac.as_deref()?.parse().ok()),
        encryption: config.encryption.clone(),
        networks: config.network.clone(),
        up: radio_up && nl.is_some_and(|nl| nl.wiphy_freq.is_some()),
    })
}

#[cfg(test)]
mod tests {
    use uniwrt_model::wifi::WifiStandard;

    use super::{Phy, by_path, standard};

    #[test]
    fn htmodes_name_their_standard() {
        assert_eq!(standard("HE80"), Some(WifiStandard::He));
        assert_eq!(standard("EHT320"), Some(WifiStandard::Eht));
        assert_eq!(standard("VHT40"), Some(WifiStandard::Vht));
        assert_eq!(standard("HT20"), Some(WifiStandard::Ht));
        assert_eq!(standard("NOHT"), Some(WifiStandard::Legacy));
        assert_eq!(standard("auto"), None);
    }

    #[test]
    fn a_path_with_a_suffix_names_a_later_phy_of_its_device() {
        let phy = |name: &str, index| Phy {
            name: name.into(),
            index,
            device: Some("/sys/devices/platform/soc/18000000.wifi".into()),
        };
        let phys = [phy("phy1", 1), phy("phy0", 0)];
        assert_eq!(
            by_path(&phys, "platform/soc/18000000.wifi").as_deref(),
            Some("phy0")
        );
        assert_eq!(
            by_path(&phys, "platform/soc/18000000.wifi+1").as_deref(),
            Some("phy1")
        );
        assert_eq!(by_path(&phys, "platform/soc/18000000.wifi+2"), None);
        assert_eq!(by_path(&phys, "pci0000:00/0000:00:01.0"), None);
    }
}
