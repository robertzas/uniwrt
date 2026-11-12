//! The `usteer` package (features/wifi.md, Steering & balancing; D-090): usteer on each node
//! that carries a steered WLAN, set by the site's profile, exchanging what it sees with the
//! other nodes' over the management network (UDP 16720, which that network's zone accepts).
//! Every threshold a profile sets is written out, because the releases' usteer defaults differ
//! (24.10's band-steers every 2 min, 25.12's every 30 s); Conservative is 24.10's stock
//! behavior. usteer tells WLANs apart only by SSID: its `ssid_list` names the steered ones,
//! and it then owns their 802.11k neighbor lists (it sets them every second), which the
//! controller's neighbor reports leave alone ([`SiteConfig::steers`]). usteer has no switch of
//! its own: with steering off on a node that holds the package, the package keeps its network
//! settings and the init script is disabled.
//!
//! [`SiteConfig::steers`]: uniwrt_model::site::SiteConfig::steers

use uniwrt_model::{facts::WifiFeature, site::steering::SteeringProfile};
use uniwrt_uci::{Package, Section, Value};

use crate::{Issue, Requirement, plan::Plan};

/// The package steering needs, with its init script.
pub(crate) const REQUIRES: Requirement = Requirement {
    package: "usteer",
    service: Some("usteer"),
};

/// The thresholds of a profile, as usteer names them: signals in dBm (negative), 0 for off;
/// intervals in ms. Conservative's are 24.10's usteer defaults.
pub fn thresholds(profile: SteeringProfile) -> &'static [(&'static str, &'static str)] {
    match profile {
        SteeringProfile::Off => &[],
        // Band steering only: 802.11v clients that stay strong for 2 min are asked up a band.
        SteeringProfile::Conservative => &[
            ("band_steering_interval", "120000"),
            ("band_steering_min_snr", "-60"),
            ("roam_scan_snr", "0"),
            ("roam_trigger_snr", "0"),
            ("signal_diff_threshold", "0"),
            ("min_snr", "0"),
            ("assoc_steering", "0"),
            ("load_kick_enabled", "0"),
        ],
        // Also a client fading below -75 dBm is sent to an AP heard 8 dB stronger (a client
        // that ignores the request is disconnected 10 s later, so it reconnects there).
        SteeringProfile::Balanced => &[
            ("band_steering_interval", "60000"),
            ("band_steering_min_snr", "-63"),
            ("roam_scan_snr", "-70"),
            ("roam_trigger_snr", "-75"),
            ("signal_diff_threshold", "8"),
            ("min_snr", "0"),
            ("assoc_steering", "0"),
            ("load_kick_enabled", "0"),
        ],
        // Sooner and on smaller differences; associations below -85 dBm are refused (and
        // clients that fall below it dropped after 10 s), an AP refuses a client another AP
        // serves better, and channels busier than 75 % shed clients.
        SteeringProfile::Aggressive => &[
            ("band_steering_interval", "30000"),
            ("band_steering_min_snr", "-66"),
            ("roam_scan_snr", "-67"),
            ("roam_trigger_snr", "-72"),
            ("signal_diff_threshold", "5"),
            ("min_snr", "-85"),
            ("min_snr_kick_delay", "10000"),
            ("assoc_steering", "1"),
            ("load_kick_enabled", "1"),
            ("load_kick_threshold", "75"),
        ],
    }
}

/// The package and whether usteer runs, or `None` where there is nothing to own: steering
/// off on the node and no package on it.
pub(crate) fn render(plan: &mut Plan<'_>, held: bool) -> Option<(Package, bool)> {
    let profile = plan.site.steering.profile;
    let on = profile != SteeringProfile::Off
        && !plan.site.steered_wlans(plan.config, plan.facts).is_empty();
    if !on && !held {
        return None;
    }
    let mut section = Section::anonymous("usteer")
        .with("network", plan.mgmt)
        .with("syslog", "1")
        .with("local_mode", "0")
        .with("ipv6", "0")
        .with("debug_level", "1");
    if on {
        for (option, value) in thresholds(profile) {
            section = section.with(option, *value);
        }
        let ssids = plan.site.steered_ssids();
        section = section.with(
            "ssid_list",
            Value::List(ssids.into_iter().map(str::to_owned).collect()),
        );
        let btm = plan
            .facts
            .capabilities
            .wifi
            .contains(&WifiFeature::BssTransition);
        if !btm {
            let message = if profile == SteeringProfile::Conservative {
                "The node's hostapd has no 802.11v, which band steering needs: Conservative \
                 steering does nothing there (install the full wpad)"
            } else {
                "The node's hostapd has no 802.11v: usteer can only disconnect clients to move \
                 them (install the full wpad)"
            };
            plan.warnings.push(Issue::new("steering", message));
        }
    }
    let mut package = Package::new("usteer");
    package.sections = vec![section];
    Some((package, on))
}
