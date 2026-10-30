//! Detected capabilities: what rendering and the agent may rely on (ARCHITECTURE.md, principle 4).

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// Features detected on the node, never inferred from its version.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Capabilities {
    /// apk on OpenWrt 25.12 and later, opkg before; `None` when neither is installed.
    pub package_manager: Option<PackageManager>,
    /// How the Ethernet switch is driven; `None` without a switch.
    pub switch: Option<SwitchKind>,
    /// What the installed hostapd and `wpa_supplicant` support; empty without them.
    pub wifi: BTreeSet<WifiFeature>,
    /// Optional software the controller uses when it is present.
    pub software: BTreeSet<Software>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum PackageManager {
    Apk,
    Opkg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SwitchKind {
    /// Distributed Switch Architecture: each port is a network device.
    Dsa,
    /// A switch configured through `swconfig` and `switch_vlan` sections.
    Swconfig,
}

/// A hostapd or `wpa_supplicant` feature, serialized as the 802.11 amendment or short name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum WifiFeature {
    /// WPA3-Personal.
    #[serde(rename = "sae")]
    Sae,
    /// Enhanced Open.
    #[serde(rename = "owe")]
    Owe,
    /// WPA-Enterprise (802.1X).
    #[serde(rename = "eap")]
    Eap,
    /// 802.11s mesh with SAE, through `wpa_supplicant`.
    #[serde(rename = "mesh")]
    Mesh,
    /// 802.11r fast transition.
    #[serde(rename = "11r")]
    FastTransition,
    /// 802.11k radio measurement (neighbor reports).
    #[serde(rename = "11k")]
    RadioMeasurement,
    /// 802.11v BSS transition management.
    #[serde(rename = "11v")]
    BssTransition,
    /// 802.11w protected management frames.
    #[serde(rename = "11w")]
    ProtectedManagementFrames,
    /// Automatic channel selection.
    #[serde(rename = "acs")]
    Acs,
    #[serde(rename = "wps")]
    Wps,
}

/// Optional software, serialized as its package name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub enum Software {
    /// The batman-adv kernel module.
    BatmanAdv,
    Batctl,
    Iw,
    /// The `iwinfo` ubus object.
    RpcdModIwinfo,
    Usteer,
    Nlbwmon,
    Lldpd,
    Iperf3,
    Luci,
}

#[cfg(test)]
mod tests {
    use serde_json::{json, to_value};

    use super::{PackageManager, Software, SwitchKind, WifiFeature};

    #[test]
    fn serde_names() {
        assert_eq!(
            to_value([PackageManager::Apk, PackageManager::Opkg]).unwrap(),
            json!(["apk", "opkg"])
        );
        assert_eq!(to_value(SwitchKind::Dsa).unwrap(), json!("dsa"));
        assert_eq!(
            to_value([
                WifiFeature::FastTransition,
                WifiFeature::ProtectedManagementFrames
            ])
            .unwrap(),
            json!(["11r", "11w"])
        );
        assert_eq!(
            to_value([Software::BatmanAdv, Software::RpcdModIwinfo]).unwrap(),
            json!(["batman-adv", "rpcd-mod-iwinfo"])
        );
    }
}
