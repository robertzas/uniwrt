//! The `usteer` package (features/wifi.md, Steering & balancing; D-090): usteer on each node
//! that carries a steered WLAN, set by the site's profile, exchanging what it sees with the
//! other nodes' over the management network (UDP 16720, which that network's zone accepts).
//! The controller decides band steering and roam suggestions (D-093) and asks usteer where it
//! hears a client; every option a profile sets is written out, because the releases' usteer
//! defaults differ (24.10's band-steers every 2 min, 25.12's every 30 s). usteer tells WLANs apart only by SSID: its `ssid_list` names the steered ones,
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

/// The thresholds of a profile, as usteer names them: signals in dBm (negative), 0 for off.
/// The controller sends band steering's and roam suggestions' transition requests itself
/// (D-093: 24.10's and 25.12's hostapd refuse usteer 2022.08's, which lack the MBO fields), so
/// usteer's own are off everywhere (`band_steering_interval`, `roam_scan_snr` and
/// `roam_trigger_snr` 0) and usteer keeps what it does without them: Aggressive's refusals,
/// minimum signal and load kicks. `signal_diff_threshold` is the margin a better candidate
/// needs, for association steering (and import tells Balanced by it).
pub fn thresholds(profile: SteeringProfile) -> &'static [(&'static str, &'static str)] {
    match profile {
        SteeringProfile::Off => &[],
        SteeringProfile::Conservative => &[
            ("band_steering_interval", "0"),
            ("roam_scan_snr", "0"),
            ("roam_trigger_snr", "0"),
            ("signal_diff_threshold", "0"),
            ("min_snr", "0"),
            ("assoc_steering", "0"),
            ("load_kick_enabled", "0"),
        ],
        SteeringProfile::Balanced => &[
            ("band_steering_interval", "0"),
            ("roam_scan_snr", "0"),
            ("roam_trigger_snr", "0"),
            ("signal_diff_threshold", "8"),
            ("min_snr", "0"),
            ("assoc_steering", "0"),
            ("load_kick_enabled", "0"),
        ],
        // Associations below -85 dBm are refused (and clients that fall below it dropped after
        // 10 s), an AP refuses a client another AP serves 5 dB better, and channels busier than
        // 75 % shed clients.
        SteeringProfile::Aggressive => &[
            ("band_steering_interval", "0"),
            ("roam_scan_snr", "0"),
            ("roam_trigger_snr", "0"),
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
            let message = if profile == SteeringProfile::Aggressive {
                "The node's hostapd has no 802.11v: clients can't be asked to move there, only \
                 refused or disconnected (install the full wpad)"
            } else {
                "The node's hostapd has no 802.11v, which steering's transition requests need: \
                 steering does nothing there (install the full wpad)"
            };
            plan.warnings.push(Issue::new("steering", message));
        }
    }
    let mut package = Package::new("usteer");
    package.sections = vec![section];
    Some((package, on))
}
