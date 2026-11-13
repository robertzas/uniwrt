//! The `radios` set: netifd's `network.wireless status` (each radio's configuration and state),
//! nl80211's interfaces (channel, width, power and addresses) and each phy's channel survey. A
//! radio's phy is the phy of its interfaces; while it has none, the one its `phy` option names or
//! whose device is its `path`. A radio's restart is pending when netifd runs another channel or
//! width than UCI saved (a channel switch, D-113); the agent reports UCI's values
//! (none for a radio with uncommitted changes), and without them nothing is pending.

use std::collections::{BTreeMap, HashMap};

use serde::Deserialize;
use uniwrt_model::{
    facts::{RadioFacts, WifiIfaceFacts, WifiIfaceMode},
    wifi::{Band, WifiStandard},
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
    /// UCI's channel and width.
    #[serde(default)]
    saved: Option<Saved>,
    #[serde(default)]
    interfaces: Vec<Iface>,
}

#[derive(Default, Deserialize)]
struct Saved {
    #[serde(default, deserialize_with = "text")]
    channel: Option<String>,
    htmode: Option<String>,
}

/// A UCI value netifd may report as a number.
fn text<'de, D: serde::Deserializer<'de>>(de: D) -> Result<Option<String>, D::Error> {
    Ok(match Option::<serde_json::Value>::deserialize(de)? {
        Some(serde_json::Value::String(text)) => Some(text),
        Some(serde_json::Value::Number(n)) => Some(n.to_string()),
        _ => None,
    })
}

#[derive(Default, Deserialize)]
struct RadioConfig {
    band: Option<String>,
    #[serde(default, deserialize_with = "text")]
    channel: Option<String>,
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
                .or_else(|| Band::of_frequency(frequency_mhz?))?;
            let survey = phy
                .as_ref()
                .and_then(|phy| raw.surveys.get(phy))
                .and_then(Option::as_ref);
            Some(RadioFacts {
                name,
                phy,
                path: radio.config.path.clone(),
                band,
                up: radio.up,
                disabled: radio.disabled,
                restart_pending: radio.up && restart_pending(&radio.config, radio.saved.as_ref()),
                channel: frequency_mhz.and_then(|mhz| band.channel(mhz)),
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

/// Whether netifd runs another channel or width than UCI saved. A value missing on either side
/// is taken as equal: only a known difference restarts a radio.
fn restart_pending(running: &RadioConfig, saved: Option<&Saved>) -> bool {
    let Some(saved) = saved else {
        return false;
    };
    let differs =
        |a: Option<&str>, b: Option<&str>| a.zip(b).is_some_and(|(a, b)| a.trim() != b.trim());
    differs(running.channel.as_deref(), saved.channel.as_deref())
        || differs(running.htmode.as_deref(), saved.htmode.as_deref())
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

    use super::{Phy, RadioConfig, Saved, by_path, restart_pending, standard};

    #[test]
    fn a_radio_restarts_pending_only_where_netifd_runs_another_channel_or_width_than_saved() {
        let running = RadioConfig {
            channel: Some("36".into()),
            htmode: Some("HE80".into()),
            ..RadioConfig::default()
        };
        let saved = |channel: Option<&str>, htmode: Option<&str>| Saved {
            channel: channel.map(Into::into),
            htmode: htmode.map(Into::into),
        };
        assert!(!restart_pending(&running, None));
        assert!(!restart_pending(
            &running,
            Some(&saved(Some("36"), Some("HE80")))
        ));
        assert!(restart_pending(
            &running,
            Some(&saved(Some("44"), Some("HE80")))
        ));
        assert!(restart_pending(
            &running,
            Some(&saved(Some("36"), Some("HE40")))
        ));
        // Unknown on one side: nothing to restart for.
        assert!(!restart_pending(&running, Some(&saved(None, None))));
        let raw: super::Radio = serde_json::from_value(serde_json::json!({
            "up": true,
            "config": {"channel": 36, "htmode": "HE80"},
            "saved": {"channel": "149", "htmode": "HE80"},
        }))
        .unwrap();
        assert!(restart_pending(&raw.config, raw.saved.as_ref()));
    }

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
