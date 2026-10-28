//! The `capabilities` set: binaries and modules found on the node, the build features that
//! hostapd and `wpa_supplicant` report (`-v<feature>`), and how the switch is driven.

use serde::Deserialize;
use serde_json::Value;
use uniwrt_model::facts::{Capabilities, PackageManager, Software, SwitchKind, WifiFeature};

#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct Raw {
    package_managers: Vec<String>,
    software: Vec<String>,
    /// None without the binary.
    hostapd: Option<Vec<String>>,
    wpa_supplicant: Option<Vec<String>>,
    /// board.json describes a switch: swconfig.
    swconfig: bool,
    /// A device is a DSA conduit.
    dsa: bool,
}

pub(super) fn parse(raw: Raw) -> Capabilities {
    let has = |name: &str| raw.package_managers.iter().any(|found| found == name);
    let package_manager = if has("apk") {
        Some(PackageManager::Apk)
    } else if has("opkg") {
        Some(PackageManager::Opkg)
    } else {
        None
    };
    let switch = if raw.swconfig {
        Some(SwitchKind::Swconfig)
    } else if raw.dsa {
        Some(SwitchKind::Dsa)
    } else {
        None
    };
    let mut wifi = std::collections::BTreeSet::new();
    let built = raw.hostapd.iter().chain(&raw.wpa_supplicant).flatten();
    wifi.extend(built.filter_map(|name| match name.as_str() {
        "sae" => Some(WifiFeature::Sae),
        "owe" => Some(WifiFeature::Owe),
        "eap" => Some(WifiFeature::Eap),
        "mesh" => Some(WifiFeature::Mesh),
        "11r" => Some(WifiFeature::FastTransition),
        "acs" => Some(WifiFeature::Acs),
        "wps" => Some(WifiFeature::Wps),
        _ => None,
    }));
    if raw.hostapd.is_some() {
        // No build switch: every hostapd OpenWrt has shipped since 23.05 (2.10 and later) has
        // them.
        wifi.extend([
            WifiFeature::RadioMeasurement,
            WifiFeature::BssTransition,
            WifiFeature::ProtectedManagementFrames,
        ]);
    }
    Capabilities {
        package_manager,
        switch,
        wifi,
        // Package names the model doesn't know (a newer agent) are skipped.
        software: raw
            .software
            .into_iter()
            .filter_map(|name| serde_json::from_value::<Software>(Value::String(name)).ok())
            .collect(),
    }
}
