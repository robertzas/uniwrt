//! A WLAN's roaming options on each of its BSSes (features/wifi.md, Roaming; D-085), each only
//! where the node's hostapd has it: 802.11r (fast transition) over the air in the WLAN's
//! mobility domain, its PMK-R0/R1 keys generated locally for WPA2 or, for SAE, sent between
//! the access points under the WLAN's FT key ([`Secrets::ft_key`](crate::Secrets::ft_key));
//! 802.11k neighbor reports (the controller fills them in, M5.2); 802.11v BSS transition with
//! WNM sleep, its keys left out of sleep responses for stations that would reinstall them.

use uniwrt_model::{
    facts::WifiFeature,
    site::wireless::{Wlan, WlanSecurity},
};
use uniwrt_uci::{Section, Value};

use crate::{Issue, ext::SectionExt, plan::Plan};

/// Any R0KH and R1KH: the access points learn each other from their frames, all trusting the
/// key (hostapd's wildcard entries, as OpenWrt writes them without lists).
const ANY_R0KH: &str = "ff:ff:ff:ff:ff:ff,*";
const ANY_R1KH: &str = "00:00:00:00:00:00,00:00:00:00:00:00";

/// `section`, a BSS of `wlan` encrypted as `encryption` (the mode render settled on), with the
/// WLAN's roaming options.
pub(crate) fn options(
    plan: &mut Plan<'_>,
    wlan: &Wlan,
    encryption: &str,
    section: Section,
) -> Section {
    let at = format!("wlans/{}/roaming", wlan.id);
    let wifi = &plan.facts.capabilities.wifi;
    let (ft, rrm, btm) = (
        wifi.contains(&WifiFeature::FastTransition),
        wifi.contains(&WifiFeature::RadioMeasurement),
        wifi.contains(&WifiFeature::BssTransition),
    );
    let mut section = section;
    if wlan.fast_roams() {
        if ft {
            section = fast(plan, wlan, encryption, section, &at);
        } else {
            warn(
                plan,
                &at,
                "The node's hostapd has no 802.11r: its clients roam without fast transition",
            );
        }
    }
    if !wlan.roaming.hints {
        return section;
    }
    if !btm {
        warn(
            plan,
            &at,
            "The node's hostapd has no 802.11v (BSS transition): install the full wpad for it",
        );
    }
    section
        .flag("ieee80211k", rrm)
        .flag("bss_transition", btm)
        .flag("wnm_sleep_mode", btm)
        .flag("wnm_sleep_mode_no_keys", btm)
}

fn fast(plan: &mut Plan<'_>, wlan: &Wlan, encryption: &str, section: Section, at: &str) -> Section {
    let section = section
        .with("ieee80211r", "1")
        .with("mobility_domain", wlan.mobility_domain())
        .with("ft_over_ds", "0");
    if encryption == "psk2" {
        return section.with("ft_psk_generate_local", "1");
    }
    // SAE derives a new PMK per association: the access points hand its keys to each other.
    let Some(key) = plan.secrets.ft_key(&wlan.id).map(str::to_owned) else {
        let issue = Issue::new(at, "The controller has no 802.11r key for this WLAN");
        if !plan.errors.contains(&issue) {
            plan.errors.push(issue);
        }
        return section;
    };
    debug_assert!(matches!(
        wlan.security,
        WlanSecurity::Wpa2Wpa3 | WlanSecurity::Wpa3
    ));
    section
        .with("ft_psk_generate_local", "0")
        .with("r0kh", Value::List(vec![format!("{ANY_R0KH},{key}")]))
        .with("r1kh", Value::List(vec![format!("{ANY_R1KH},{key}")]))
}

/// Warns once per node, however many radios carry the WLAN.
fn warn(plan: &mut Plan<'_>, at: &str, message: &str) {
    let issue = Issue::new(at, message);
    if !plan.warnings.contains(&issue) {
        plan.warnings.push(issue);
    }
}
