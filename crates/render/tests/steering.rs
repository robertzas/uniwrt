//! Steering (features/wifi.md, Steering & balancing; D-090): usteer's package per profile, where
//! it runs, and what is left when steering goes off.

// Tests and their fixtures may panic on what they set up.
#![allow(clippy::unwrap_used)]

mod support;

use std::{collections::BTreeMap, fmt::Write as _};

use insta::assert_snapshot;
use uniwrt_model::{
    facts::WifiFeature,
    node::NodeRole,
    site::{SiteConfig, node::ManagedPackage, steering::SteeringProfile},
};
use uniwrt_render::Rendered;

use support::*;

fn ok(result: Result<Rendered, Vec<uniwrt_render::Issue>>) -> Rendered {
    result.unwrap_or_else(|issues| panic!("{issues:?}"))
}

/// The home site with `profile`, its guest WLAN left out of steering.
fn steered(profile: SteeringProfile) -> SiteConfig {
    let mut site = home_site();
    site.steering.profile = profile;
    site.wlans[2].roaming.steer = false;
    site
}

/// What steering renders: the service, the requirement, the warnings and the package.
fn usteer(rendered: &Rendered) -> String {
    let mut out = format!("service: {:?}\n", rendered.services.get("usteer"));
    let requires: Vec<&str> = rendered.requires.iter().map(|r| r.package).collect();
    let _ = writeln!(out, "requires: {}", requires.join(", "));
    for issue in rendered.warnings.iter().filter(|i| i.path == "steering") {
        let _ = writeln!(out, "warning: {}", issue.message);
    }
    if let Some(package) = rendered.packages.get(&ManagedPackage::Usteer) {
        out.push_str(&package.to_file());
    }
    out
}

#[test]
fn each_profile_renders_its_thresholds_for_the_steered_ssids() {
    let facts = gateway_facts();
    let mut all = String::new();
    for profile in SteeringProfile::ALL {
        let rendered = ok(run(&steered(profile), AP, NodeRole::Ap, &facts));
        let _ = write!(all, "## {profile:?}\n{}\n", usteer(&rendered));
    }
    assert_snapshot!("steering_profiles", all);
}

#[test]
fn off_leaves_usteer_alone_unless_the_node_holds_it() {
    let facts = gateway_facts();
    let site = steered(SteeringProfile::Off);
    let rendered = ok(run(&site, AP, NodeRole::Ap, &facts));
    assert!(!rendered.packages.contains_key(&ManagedPackage::Usteer));
    assert!(!rendered.services.contains_key("usteer"));
    // Held: its network settings stay, the thresholds go, and the service stops.
    let mut current = stock();
    current.insert(
        "usteer".to_owned(),
        uniwrt_uci::parse(
            "usteer",
            "config usteer\n\toption network 'lan'\n\toption roam_trigger_snr '-75'\n",
        )
        .unwrap(),
    );
    let rendered = ok(run_on(&site, AP, NodeRole::Ap, &facts, &current));
    assert_snapshot!("steering_off_held", usteer(&rendered));
}

#[test]
fn usteer_runs_only_where_a_steered_wlan_is_on_air() {
    let site = steered(SteeringProfile::Balanced);
    // No radios (an x86 gateway): nothing to steer.
    let mut wired = site.clone();
    wired.mesh.enabled = false;
    let rendered = ok(run(&wired, GATEWAY, NodeRole::Gateway, &x86_facts()));
    assert!(!rendered.packages.contains_key(&ManagedPackage::Usteer));
    // Every WLAN left out, or the only steered one off.
    let mut none = site.clone();
    none.wlans[0].roaming.steer = false;
    none.wlans[1].enabled = false;
    let facts = gateway_facts();
    let rendered = ok(run(&none, AP, NodeRole::Ap, &facts));
    assert!(!rendered.packages.contains_key(&ManagedPackage::Usteer));
    let config = managed(AP);
    assert!(site.steers(&config, &facts));
    assert!(!none.steers(&config, &facts));
    assert!(!site.steers(&uniwrt_model::site::node::NodeConfig::new(AP), &facts));
    assert_eq!(site.steered_ssids(), ["Home", "Things"]);
}

#[test]
fn without_802_11v_steering_warns_what_it_can_still_do() {
    let mut facts = gateway_facts();
    facts
        .capabilities
        .wifi
        .retain(|f| *f != WifiFeature::BssTransition);
    let mut warnings = BTreeMap::new();
    for profile in [SteeringProfile::Balanced, SteeringProfile::Aggressive] {
        let rendered = ok(run(&steered(profile), AP, NodeRole::Ap, &facts));
        let found: Vec<String> = rendered
            .warnings
            .iter()
            .filter(|i| i.path == "steering")
            .map(|i| i.message.clone())
            .collect();
        warnings.insert(format!("{profile:?}"), found);
    }
    assert!(warnings["Balanced"][0].contains("does nothing"));
    assert!(warnings["Aggressive"][0].contains("only refused or disconnected"));
}
