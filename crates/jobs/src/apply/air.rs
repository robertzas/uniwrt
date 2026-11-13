//! The radios an apply moves in the Channels round (features/wifi.md, RF planner: Apply;
//! M6.1 b1b, D-113), pure: a node whose `wireless` change is only its radios' `channel`,
//! `htmode` and `txpower` waits until the fleet's transactions are confirmed, then moves one
//! node at a time.
//!
//! The power goes in the node's first transaction (an ordinary reload: hostapd keeps the radio
//! running when only its power changes, ✅ 24.10 and 25.12). The Channels round then moves the
//! channels: where every radio that moves can announce the switch ([`uniwrt_rf::switch`]), its
//! transaction installs `wireless` without reloading it and hostapd's `switch_chan` moves the
//! radios with their clients; else the transaction reloads, as any other. A radio carrying the
//! mesh keeps the node out: the mesh's cutover is the fleet's (M6.1 b2).

use std::collections::BTreeMap;

use uniwrt_model::{
    facts::{RadioFacts, WifiIfaceMode},
    site::node::ManagedPackage,
    wifi::{ChannelWidth, WifiStandard},
};
use uniwrt_rf::switch::{Switch, switch};
use uniwrt_uci::Package;

use super::{
    plan::{Part, Report},
    round::Round,
};

/// The options of a `wifi-device` the Channels round may change.
const OPTIONS: [&str; 3] = ["channel", "htmode", "txpower"];

/// What the Channels round does on one node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Air {
    /// The node's first transaction: `wireless` as it holds it with the rendered powers;
    /// `None` when no power changes.
    pub(crate) first: Option<Package>,
    /// The rendered `wireless`, the Channels round's transaction.
    pub(crate) wireless: Package,
    pub(crate) moves: Vec<Move>,
}

impl Air {
    /// Every radio that moves can switch: the transaction reloads nothing.
    pub(crate) fn live(&self) -> bool {
        self.moves.iter().all(|m| m.switch.is_ok())
    }
}

/// One radio whose channel or width changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Move {
    /// UCI section, such as `radio1`.
    pub(crate) radio: String,
    /// The channel it moves to (0 for automatic) and the width.
    pub(crate) channel: u16,
    pub(crate) width: ChannelWidth,
    /// The access point whose hostapd switches the radio, and the switch; else why it
    /// reloads instead.
    pub(crate) switch: Result<(String, Switch), &'static str>,
}

/// What the Channels round does to a node whose `wireless` goes from `before` to `after`;
/// `None` when the change isn't only radios' channels and widths (power alone reloads with
/// the others: hostapd keeps running).
pub(crate) fn air(before: &Package, after: &Package, radios: &[RadioFacts]) -> Option<Air> {
    let diff = uniwrt_uci::diff(before, after);
    let mut first = before.clone();
    let mut power = false;
    let mut moves = Vec::new();
    for section in &diff.sections {
        let (Some(was), Some(now)) = (&section.before, &section.after) else {
            return None;
        };
        let name = now.name.as_deref()?;
        if section.moved
            || was.kind != "wifi-device"
            || now.kind != "wifi-device"
            || was.name != now.name
            || section
                .options
                .iter()
                .any(|o| !OPTIONS.contains(&o.name.as_str()))
            || meshes(name, before, after, radios)
        {
            return None;
        }
        let rendered = after.section(name)?;
        if section.options.iter().any(|o| o.name == "txpower") {
            power = true;
            let held = first.section_mut(name)?;
            match rendered.get("txpower") {
                Some(value) => held.set("txpower", value.clone()),
                None => {
                    held.remove("txpower");
                }
            }
        }
        if section.options.iter().any(|o| o.name != "txpower") {
            let facts = radios.iter().find(|r| r.name == name);
            moves.push(movement(name, rendered, facts));
        }
    }
    if moves.is_empty() {
        return None;
    }
    Some(Air {
        first: power.then_some(first),
        wireless: after.clone(),
        moves,
    })
}

/// Leaves the channels to the Channels round where nothing but the radios' channels, widths
/// and powers changes (D-113): this transaction carries the powers alone, if any.
pub(super) fn split(
    rendered: &BTreeMap<ManagedPackage, Package>,
    current: &BTreeMap<String, Package>,
    report: &Report<'_>,
    part: &mut Part,
) {
    let empty = Package::new("wireless");
    let only_wireless = part.services.is_empty()
        && part.installs.is_none()
        && part.address.is_none()
        && part.bridge.is_none()
        && part.next.is_empty()
        && part.files.iter().all(|(name, _)| name == "wireless");
    let Some(air) = rendered
        .get(&ManagedPackage::Wireless)
        .filter(|_| only_wireless)
        .and_then(|after| {
            let before = current.get("wireless").unwrap_or(&empty);
            air(before, after, &report.facts.radios)
        })
    else {
        return;
    };
    part.files.clear();
    if let Some(first) = &air.first {
        let file = first.to_file();
        part.held.insert("wireless".to_owned(), file.clone());
        part.files.push(("wireless".to_owned(), file));
    } else if let Some(Some(text)) = report.files.get("wireless") {
        part.held.insert("wireless".to_owned(), text.clone());
    }
    part.next.push(Round::Air);
    part.plan.air = true;
    part.air = Some(air);
}

/// Whether radio `name` carries the mesh, before or after.
fn meshes(name: &str, before: &Package, after: &Package, radios: &[RadioFacts]) -> bool {
    let configured = |package: &Package| {
        package.sections_of("wifi-iface").any(|iface| {
            iface.option("device") == Some(name) && iface.option("mode") == Some("mesh")
        })
    };
    configured(before)
        || configured(after)
        || radios
            .iter()
            .filter(|r| r.name == name)
            .any(|r| r.ifaces.iter().any(|i| i.mode == WifiIfaceMode::Mesh))
}

fn movement(radio: &str, rendered: &uniwrt_uci::Section, facts: Option<&RadioFacts>) -> Move {
    let channel = rendered
        .option("channel")
        .and_then(|c| c.parse::<u16>().ok());
    let mode = rendered.option("htmode").and_then(htmode);
    let switch = (|| {
        let facts = facts.ok_or("the node doesn't report it")?;
        if !facts.up {
            return Err("it is down");
        }
        let channel = channel.ok_or("its channel is automatic")?;
        let (standard, width) = mode.ok_or("its mode is unknown")?;
        if facts.ifaces.iter().any(|i| i.mode != WifiIfaceMode::Ap) {
            return Err("it carries more than access points");
        }
        let ifname = facts
            .ifaces
            .iter()
            .filter(|i| i.up)
            .find_map(|i| i.name.clone())
            .ok_or("none of its access points is up")?;
        let switch = switch(facts.band, channel, width, standard)
            .ok_or("a channel switch can't announce that channel and width")?;
        Ok((ifname, switch))
    })();
    Move {
        radio: radio.to_owned(),
        channel: channel.unwrap_or(0),
        width: mode.map_or(ChannelWidth::Mhz20, |(_, width)| width),
        switch,
    }
}

/// The standard and width of a UCI `htmode`, such as `HE80`, `HT40+` or `NOHT`.
fn htmode(mode: &str) -> Option<(WifiStandard, ChannelWidth)> {
    if mode == "NOHT" {
        return Some((WifiStandard::Legacy, ChannelWidth::Mhz20));
    }
    let (standard, rest) = [
        ("EHT", WifiStandard::Eht),
        ("HE", WifiStandard::He),
        ("VHT", WifiStandard::Vht),
        ("HT", WifiStandard::Ht),
    ]
    .into_iter()
    .find_map(|(prefix, standard)| mode.strip_prefix(prefix).map(|rest| (standard, rest)))?;
    let mhz: u16 = rest.trim_end_matches(['+', '-']).parse().ok()?;
    Some((standard, ChannelWidth::try_from(mhz).ok()?))
}

#[cfg(test)]
#[path = "air_tests.rs"]
mod tests;
