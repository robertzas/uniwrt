//! WLANs, the mesh and the radio policy.

use std::collections::BTreeSet;

use uniwrt_core::NodeId;

use super::{Issues, names};
use crate::{
    site::{
        SiteConfig,
        steering::SteeringProfile,
        wireless::{Channel, NodeScope, Schedule, TxPower, Wlan, WlanSecurity},
    },
    wifi::{Band, ChannelWidth},
};

pub(super) fn wlans(config: &SiteConfig, nodes: &BTreeSet<NodeId>, issues: &mut Issues) {
    let mut ids = BTreeSet::new();
    for wlan in &config.wlans {
        let at = format!("wlans/{}", wlan.id);
        if !names::is_id(&wlan.id) {
            issues.error(
                &at,
                format!(
                    "WLAN id {:?} must be a lowercase letter, then up to 11 lowercase letters, \
                     digits or underscores",
                    wlan.id
                ),
            );
        }
        if !ids.insert(wlan.id.as_str()) {
            issues.error(&at, format!("Two WLANs have the id {}", wlan.id));
        }
        if wlan.ssid.is_empty() || wlan.ssid.len() > 32 {
            issues.error(format!("{at}/ssid"), "An SSID is 1 to 32 bytes");
        }
        if !config
            .networks
            .iter()
            .any(|network| network.id == wlan.network)
        {
            issues.error(
                format!("{at}/network"),
                format!("There is no network {}", wlan.network),
            );
        }
        if wlan.security.needs_passphrase() && wlan.passphrase.is_none() {
            issues.error(
                format!("{at}/passphrase"),
                "This security mode needs a passphrase",
            );
        }
        if wlan.bands.is_empty() {
            issues.error(
                format!("{at}/bands"),
                "A WLAN broadcasts on one band at least",
            );
        }
        let mut bands = BTreeSet::new();
        for &band in &wlan.bands {
            if !bands.insert(band) {
                issues.error(
                    format!("{at}/bands"),
                    format!("{} is listed twice", band_name(band)),
                );
            }
            if band == Band::G60 {
                issues.error(format!("{at}/bands"), "60 GHz is not supported");
            }
        }
        if bands.contains(&Band::G6)
            && !matches!(
                wlan.security,
                WlanSecurity::Wpa3 | WlanSecurity::Owe | WlanSecurity::OweTransition
            )
        {
            issues.error(
                format!("{at}/security"),
                "6 GHz requires WPA3 or OWE: pick one, or leave 6 GHz out",
            );
        }
        if wlan.security == WlanSecurity::Open {
            issues.warn(
                format!("{at}/security"),
                "Open and unencrypted: OWE keeps an open network private",
            );
        }
        if wlan
            .max_clients
            .is_some_and(|max| !(1..=MAX_CLIENTS).contains(&max))
        {
            issues.error(
                format!("{at}/max_clients"),
                format!("A radio takes 1 to {MAX_CLIENTS} clients"),
            );
        }
        if wlan.security == WlanSecurity::OweTransition && wlan.ssid.len() > 29 {
            issues.error(
                format!("{at}/ssid"),
                "In OWE transition mode an SSID is 29 bytes at most: the encrypted one adds OWE",
            );
        }
        roaming(wlan, &at, issues);
        if let Some(schedule) = &wlan.schedule {
            self::schedule(schedule, &format!("{at}/schedule"), issues);
        }
        scope(&wlan.nodes, &format!("{at}/nodes"), nodes, issues);
    }
}

fn roaming(wlan: &Wlan, at: &str, issues: &mut Issues) {
    match (wlan.roaming.fast, wlan.security) {
        (Some(true), security) if !security.needs_passphrase() => issues.warn(
            format!("{at}/roaming"),
            "Fast roaming (802.11r) needs a passphrase: this security mode goes without it",
        ),
        (Some(true), WlanSecurity::Wpa2Wpa3 | WlanSecurity::Wpa3) => issues.warn(
            format!("{at}/roaming"),
            "Some clients fail to join WPA3 networks with fast roaming (802.11r): turn it off \
             if a device can't connect",
        ),
        _ => {}
    }
}

/// hostapd's `max_num_sta` ends at its association IDs.
const MAX_CLIENTS: u16 = 2007;

fn schedule(schedule: &Schedule, at: &str, issues: &mut Issues) {
    if schedule.spans.is_empty() {
        issues.error(
            at,
            "A schedule without times is never on: add a time, or turn the WLAN off",
        );
    }
    for (nth, span) in schedule.spans.iter().enumerate() {
        let at = format!("{at}/spans/{nth}");
        if span.days.is_empty() {
            issues.error(&at, "A time is on one day at least");
        }
        let mut days = BTreeSet::new();
        if !span.days.iter().all(|day| days.insert(day)) {
            issues.error(&at, "A day is listed twice");
        }
        if span.from >= 1440 || span.to >= 1440 {
            issues.error(&at, "Times are within a day");
        }
    }
}

fn scope(scope: &NodeScope, at: &str, nodes: &BTreeSet<NodeId>, issues: &mut Issues) {
    let (NodeScope::Only(listed) | NodeScope::Except(listed)) = scope else {
        return;
    };
    for node in listed.iter().filter(|node| !nodes.contains(node)) {
        issues.warn(at, format!("Node {node} is not adopted"));
    }
    if matches!(scope, NodeScope::Only(listed) if listed.is_empty()) {
        issues.warn(at, "No node is in scope");
    }
}

pub(super) fn mesh(config: &SiteConfig, issues: &mut Issues) {
    let mesh = &config.mesh;
    if !mesh.enabled {
        return;
    }
    if mesh.mesh_id.is_empty() || mesh.mesh_id.len() > 32 {
        issues.error("mesh/mesh_id", "A mesh ID is 1 to 32 bytes");
    }
    if mesh.key.is_none() {
        issues.error("mesh/key", "The mesh needs a key");
    }
    if mesh.band == Some(Band::G60) {
        issues.error("mesh/band", "60 GHz is not supported");
    }
    if !(1..=4094).contains(&mesh.wired_vlan) {
        issues.error("mesh/wired_vlan", "A VLAN ID is 1 to 4094");
    } else if let Some(network) = config
        .networks
        .iter()
        .find(|n| n.vlan == Some(mesh.wired_vlan))
    {
        issues.error(
            "mesh/wired_vlan",
            format!(
                "{} uses VLAN {}, which the wired backhaul takes",
                network.name, mesh.wired_vlan
            ),
        );
    }
}

pub(super) fn radio(config: &SiteConfig, issues: &mut Issues) {
    let mut bands = BTreeSet::new();
    for policy in &config.radio.bands {
        let at = format!("radio/bands/{}", band_name(policy.band));
        if !bands.insert(policy.band) {
            issues.error(&at, "The band has two policies");
        }
        if policy.band == Band::G60 {
            issues.error(&at, "60 GHz is not supported");
            continue;
        }
        if let Channel::Fixed(channel) = policy.channel
            && !channel_fits(policy.band, channel)
        {
            issues.error(
                format!("{at}/channel"),
                format!("{channel} is not a {} channel", band_name(policy.band)),
            );
        }
        if !width_fits(policy.band, policy.width) {
            issues.error(
                format!("{at}/width"),
                format!("{} channels are not that wide", band_name(policy.band)),
            );
        }
        tx_power(policy.tx_power, &format!("{at}/tx_power"), issues);
    }
}

pub(super) fn tx_power(power: TxPower, at: &str, issues: &mut Issues) {
    if let TxPower::Dbm(dbm) = power
        && !(1..=30).contains(&dbm)
    {
        issues.error(at, "Transmit power is 1 to 30 dBm");
    }
}

/// Whether `channel` is a 20 MHz channel number of `band`.
fn channel_fits(band: Band, channel: u16) -> bool {
    match band {
        Band::G2 => (1..=14).contains(&channel),
        Band::G5 => (32..=177).contains(&channel),
        Band::G6 => (1..=233).contains(&channel) && channel % 4 == 1,
        Band::G60 => (1..=6).contains(&channel),
    }
}

fn width_fits(band: Band, width: ChannelWidth) -> bool {
    match band {
        Band::G2 => width <= ChannelWidth::Mhz40,
        Band::G5 => width <= ChannelWidth::Mhz160,
        Band::G6 | Band::G60 => true,
    }
}

fn band_name(band: Band) -> &'static str {
    match band {
        Band::G2 => "2.4 GHz",
        Band::G5 => "5 GHz",
        Band::G6 => "6 GHz",
        Band::G60 => "60 GHz",
    }
}

/// Steering: usteer tells WLANs apart by SSID only (its `ssid_list`), so WLANs of one SSID
/// steer alike; a profile with no WLAN to steer does nothing.
pub(super) fn steering(config: &SiteConfig, issues: &mut Issues) {
    if config.steering.profile == SteeringProfile::Off {
        return;
    }
    if !config.wlans.iter().any(|w| w.enabled && w.roaming.steer) {
        issues.warn("steering", "No WLAN takes part in steering");
    }
    for (i, wlan) in config.wlans.iter().enumerate() {
        let other = config.wlans[..i]
            .iter()
            .find(|o| o.ssid == wlan.ssid && o.roaming.steer != wlan.roaming.steer);
        if let Some(other) = other {
            issues.warn(
                format!("wlans/{}/roaming", wlan.id),
                format!(
                    "WLAN {} has the same SSID and steers {}: usteer steers both or neither",
                    other.id,
                    if other.roaming.steer {
                        "its clients"
                    } else {
                        "none"
                    }
                ),
            );
        }
    }
}
