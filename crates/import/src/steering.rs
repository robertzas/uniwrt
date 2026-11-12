//! Steering (features/wifi.md, Steering & balancing; D-090): a node that holds usteer's package
//! runs it (installing it enables it), so the site steers with the profile nearest to what the
//! first such node's options do: Aggressive where it kicks or refuses clients (on load, by
//! association steering or below a minimum signal), Balanced where it triggers roams, else
//! Conservative (usteer's defaults band-steer only). Its
//! `ssid_list`, when it has one, leaves out the WLANs whose SSID it doesn't name. Without
//! usteer on any node the site keeps the steering it had.

use uniwrt_model::site::{SiteConfig, steering::SteeringProfile};
use uniwrt_uci::Section;

use crate::{Source, read};

pub(crate) fn import(site: &mut SiteConfig, sources: &[Source<'_>]) {
    let Some(section) = sources
        .iter()
        .find_map(|s| read::first(s.packages, "usteer", "usteer"))
    else {
        return;
    };
    site.steering.profile = nearest(section);
    let listed = read::items(section, "ssid_list");
    if !listed.is_empty() {
        for wlan in &mut site.wlans {
            wlan.roaming.steer = listed.contains(&wlan.ssid.as_str());
        }
    }
}

fn nearest(section: &Section) -> SteeringProfile {
    let set = |option: &str| section.option(option).is_some_and(|v| v != "0");
    if set("load_kick_enabled") || set("assoc_steering") || set("min_snr") {
        SteeringProfile::Aggressive
    } else if set("roam_trigger_snr") || set("roam_scan_snr") {
        SteeringProfile::Balanced
    } else {
        SteeringProfile::Conservative
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::fmt::Write as _;

    use uniwrt_model::site::steering::SteeringProfile;

    use super::nearest;

    fn usteer(options: &str) -> SteeringProfile {
        let text = format!("config usteer\n\toption network 'lan'\n{options}");
        let package = uniwrt_uci::parse("usteer", &text).unwrap();
        nearest(&package.sections[0])
    }

    #[test]
    fn each_profile_comes_back_and_stock_usteer_is_conservative() {
        assert_eq!(usteer(""), SteeringProfile::Conservative);
        for profile in [
            SteeringProfile::Conservative,
            SteeringProfile::Balanced,
            SteeringProfile::Aggressive,
        ] {
            let mut options = String::new();
            for (option, value) in uniwrt_render::steering_thresholds(profile) {
                let _ = writeln!(options, "\toption {option} '{value}'");
            }
            assert_eq!(usteer(&options), profile);
        }
        // Roam triggers alone are nearer Balanced; a kick on load nearer Aggressive.
        assert_eq!(
            usteer("\toption roam_trigger_snr '-75'\n\toption signal_diff_threshold '8'\n"),
            SteeringProfile::Balanced
        );
        assert_eq!(
            usteer(
                "\toption load_kick_enabled '1'\n\toption assoc_steering '1'\n\
                 \toption min_snr '-85'\n"
            ),
            SteeringProfile::Aggressive
        );
    }
}
