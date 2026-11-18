//! A node's device replaced (features/lifecycle.md, Replace / RMA / decommission; D-131): the
//! new device takes over the node's settings. Radio overrides and the RF plan follow the band
//! (the n-th radio of a band on the old device becomes the n-th of that band on the new one),
//! port profiles the port's name, DHCP reservations the new MAC; what has no match is dropped
//! and listed. Raw sections described the old device's own configuration and go (Manage
//! imports the new device's); the managed packages and the hostname stay.

use uniwrt_core::{MacAddr, NodeId};

use super::SiteConfig;
use crate::{facts::NodeFacts, wifi::Band};

/// What the remap needs to know about a device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub mac: MacAddr,
    /// Its radios (`wifi-device` names) and their bands, in the order the facts list them.
    pub radios: Vec<(String, Band)>,
    /// The ports a profile may name (render's `port_names`).
    pub ports: Vec<String>,
}

impl Device {
    pub fn new(mac: MacAddr, facts: &NodeFacts, ports: Vec<String>) -> Self {
        Self {
            mac,
            radios: facts
                .radios
                .iter()
                .map(|radio| (radio.name.clone(), radio.band))
                .collect(),
            ports,
        }
    }

    /// The radio taking over `old`'s radio `name` on this device, by band and rank.
    fn heir(&self, old: &Self, name: &str) -> Option<&str> {
        let (rank, band) = match old.radios.iter().position(|(radio, _)| radio == name) {
            Some(at) => {
                let band = old.radios[at].1;
                let rank = old.radios[..at].iter().filter(|(_, b)| *b == band).count();
                (rank, band)
            }
            // Unknown to the old facts: only the same radio, by name, can take it.
            None => {
                return self
                    .radios
                    .iter()
                    .find(|(radio, _)| radio == name)
                    .map(|(radio, _)| radio.as_str());
            }
        };
        self.radios
            .iter()
            .filter(|(_, b)| *b == band)
            .nth(rank)
            .map(|(radio, _)| radio.as_str())
    }
}

/// What the new device couldn't take over.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Dropped {
    /// Radios whose overrides or planned channel went, as `radio1 (5g)`.
    pub radios: Vec<String>,
    /// Ports whose profile went.
    pub ports: Vec<String>,
    /// How many raw sections went.
    pub raw: usize,
}

impl Dropped {
    pub fn is_empty(&self) -> bool {
        self.radios.is_empty() && self.ports.is_empty() && self.raw == 0
    }

    /// One line per kind, for people.
    pub fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if !self.radios.is_empty() {
            lines.push(format!(
                "no radio of the band of {}: its settings are dropped",
                self.radios.join(", ")
            ));
        }
        if !self.ports.is_empty() {
            lines.push(format!(
                "no port named {}: the profile is dropped",
                self.ports.join(", ")
            ));
        }
        if self.raw > 0 {
            lines.push(format!(
                "{} raw section{} of the old device dropped",
                self.raw,
                if self.raw == 1 { "" } else { "s" }
            ));
        }
        lines
    }
}

/// The result of [`replace_node`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Replaced {
    /// Whether the document changed.
    pub changed: bool,
    pub dropped: Dropped,
}

/// Moves what `config` says about `node` from the `old` device to the `new` one.
pub fn replace_node(config: &mut SiteConfig, node: NodeId, old: &Device, new: &Device) -> Replaced {
    let mut out = Replaced::default();
    let band = |name: &str| {
        old.radios
            .iter()
            .find(|(radio, _)| radio == name)
            .map_or_else(
                || name.to_owned(),
                |(radio, band)| format!("{radio} ({})", band_name(*band)),
            )
    };
    let drop_radio = |dropped: &mut Dropped, name: &str| {
        let label = band(name);
        if !dropped.radios.contains(&label) {
            dropped.radios.push(label);
        }
    };
    if let Some(entry) = config.nodes.iter_mut().find(|entry| entry.node == node) {
        let before = entry.clone();
        entry.radios.retain_mut(|radio| {
            if let Some(heir) = new.heir(old, &radio.radio) {
                heir.clone_into(&mut radio.radio);
                true
            } else {
                drop_radio(&mut out.dropped, &radio.radio);
                false
            }
        });
        entry.ports.retain(|profile| {
            let kept = new.ports.contains(&profile.port);
            if !kept {
                out.dropped.ports.push(profile.port.clone());
            }
            kept
        });
        out.dropped.raw = entry.raw.len();
        entry.raw.clear();
        out.changed |= *entry != before;
    }
    let plan = &mut config.radio.plan.radios;
    let before = plan.clone();
    plan.retain_mut(|planned| {
        if planned.node != node {
            return true;
        }
        match new.heir(old, &planned.radio) {
            Some(heir)
                if new
                    .radios
                    .iter()
                    .any(|(r, b)| r == heir && *b == planned.band) =>
            {
                heir.clone_into(&mut planned.radio);
                true
            }
            _ => {
                drop_radio(&mut out.dropped, &planned.radio);
                false
            }
        }
    });
    out.changed |= *plan != before;
    if old.mac != new.mac {
        for network in &mut config.networks {
            if let Some(dhcp) = network.ipv4.as_mut().and_then(|v4| v4.dhcp.as_mut()) {
                for reservation in &mut dhcp.reservations {
                    if reservation.mac == old.mac {
                        reservation.mac = new.mac;
                        out.changed = true;
                    }
                }
            }
        }
    }
    out
}

const fn band_name(band: Band) -> &'static str {
    match band {
        Band::G2 => "2.4 GHz",
        Band::G5 => "5 GHz",
        Band::G6 => "6 GHz",
        Band::G60 => "60 GHz",
    }
}

#[cfg(test)]
#[path = "replace_tests.rs"]
mod tests;
