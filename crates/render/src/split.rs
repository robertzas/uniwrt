//! Splitting a node's `network` package into the WAN and the rest, so an apply that changes
//! the WAN with other things can change it last, in its own transaction (features/lifecycle.md,
//! Make-before-break; D-081). The WAN is its `wan*` interfaces (`wan`, `wan6`, `wan_6`, the
//! failover's `wanb`) and the `device` sections only they stand on: the device each names, and
//! the devices those are built on (`ifname`), unless another interface or the bridge uses one.

use std::collections::BTreeSet;

use uniwrt_uci::{Package, Section, Value};

/// Whether a `network` interface section carries the WAN.
pub fn wan_interface(section: &Section) -> bool {
    section.kind == "interface"
        && section
            .name
            .as_deref()
            .is_some_and(|n| n.starts_with("wan"))
}

fn items(value: Option<&Value>) -> impl Iterator<Item = &str> {
    value
        .map(Value::items)
        .into_iter()
        .flat_map(|items| items.iter().map(String::as_str))
}

/// The indices of `package`'s WAN sections: the `wan*` interfaces and the devices only they use.
pub fn wan_sections(package: &Package) -> Vec<usize> {
    let device_of = |section: &Section| {
        section
            .option("device")
            .map(|device| device.trim_start_matches('@').to_owned())
    };
    let mut wan_devices: BTreeSet<String> = package
        .sections
        .iter()
        .filter(|section| wan_interface(section))
        .filter_map(device_of)
        .collect();
    // The devices every other interface and the bridges stand on stay with the rest.
    let shared: BTreeSet<String> = package
        .sections
        .iter()
        .filter(|section| section.kind == "interface" && !wan_interface(section))
        .filter_map(device_of)
        .chain(
            package
                .sections_of("device")
                .filter(|section| section.option("type") == Some("bridge"))
                .flat_map(|section| items(section.get("ports")).map(str::to_owned)),
        )
        .collect();
    // Follow `ifname` down: a VLAN device on the WAN port takes the port's own section along.
    loop {
        let more: Vec<String> = package
            .sections_of("device")
            .filter(|section| {
                section
                    .option("name")
                    .is_some_and(|name| wan_devices.contains(name))
            })
            .filter_map(|section| section.option("ifname"))
            .filter(|parent| !wan_devices.contains(*parent) && !shared.contains(*parent))
            .map(str::to_owned)
            .collect();
        if more.is_empty() {
            break;
        }
        wan_devices.extend(more);
    }
    package
        .sections
        .iter()
        .enumerate()
        .filter(|(_, section)| {
            wan_interface(section)
                || (section.kind == "device"
                    && section
                        .option("name")
                        .is_some_and(|name| wan_devices.contains(name) && !shared.contains(name)))
        })
        .map(|(i, _)| i)
        .collect()
}

/// `rendered` with its WAN as `current` has it: each WAN section of `rendered` replaced in place
/// by `current`'s of the same identity (a named section's name, a device's `name`), the ones
/// `rendered` lacks appended, and the ones `current` lacks left out.
pub fn without_wan(rendered: &Package, current: &Package) -> Package {
    let identity = |section: &Section| {
        section
            .name
            .clone()
            .or_else(|| section.option("name").map(str::to_owned))
            .map(|id| (section.kind.clone(), id))
    };
    let current_wan: Vec<&Section> = wan_sections(current)
        .into_iter()
        .map(|i| &current.sections[i])
        .collect();
    let rendered_wan: BTreeSet<usize> = wan_sections(rendered).into_iter().collect();
    let mut used = vec![false; current_wan.len()];
    let mut package = Package::new(rendered.name.clone());
    for (i, section) in rendered.sections.iter().enumerate() {
        if !rendered_wan.contains(&i) {
            package.sections.push(section.clone());
            continue;
        }
        let id = identity(section);
        if let Some(j) = (0..current_wan.len())
            .find(|&j| !used[j] && id.is_some() && identity(current_wan[j]) == id)
        {
            used[j] = true;
            package.sections.push(current_wan[j].clone());
        }
    }
    for (j, section) in current_wan.iter().enumerate() {
        if !used[j] {
            package.sections.push((*section).clone());
        }
    }
    package
}

#[cfg(test)]
mod tests {
    use uniwrt_uci::parse;

    use super::{wan_sections, without_wan};

    const CURRENT: &str = "
config interface 'loopback'
	option device 'lo'
config device
	option name 'br-lan'
	option type 'bridge'
	list ports 'lan1'
	list ports 'lan2'
config interface 'lan'
	option device 'br-lan'
	option proto 'static'
	list ipaddr '192.168.1.1/24'
config interface 'wan'
	option device 'wan'
	option proto 'dhcp'
config interface 'wan6'
	option device 'wan'
	option proto 'dhcpv6'
";

    const RENDERED: &str = "
config interface 'loopback'
	option device 'lo'
config device
	option type '8021q'
	option ifname 'wan'
	option vid '201'
	option name 'wan.201'
config device
	option name 'br-lan'
	option type 'bridge'
	list ports 'lan1'
	list ports 'lan2'
config interface 'lan'
	option device 'br-lan'
	option proto 'static'
	list ipaddr '10.0.0.1/24'
config interface 'wan'
	option device 'wan.201'
	option proto 'pppoe'
	option username 'u'
config interface 'wan6'
	option device '@wan'
	option proto 'dhcpv6'
config interface 'wanb'
	option device 'lan2'
	option proto 'dhcp'
";

    #[test]
    fn the_wan_is_its_interfaces_and_the_devices_only_they_stand_on() {
        let rendered = parse("network", RENDERED).unwrap();
        let wan: Vec<String> = wan_sections(&rendered)
            .into_iter()
            .map(|i| {
                let s = &rendered.sections[i];
                s.name
                    .clone()
                    .unwrap_or_else(|| s.option("name").unwrap().to_owned())
            })
            .collect();
        // `wanb` sits on a bridge port: the port's device section (none here) would stay.
        assert_eq!(wan, ["wan.201", "wan", "wan6", "wanb"]);
        let current = parse("network", CURRENT).unwrap();
        assert_eq!(wan_sections(&current), [3, 4]);
    }

    #[test]
    fn without_the_wan_keeps_the_current_one_in_place() {
        let rendered = parse("network", RENDERED).unwrap();
        let current = parse("network", CURRENT).unwrap();
        let variant = without_wan(&rendered, &current);
        let names: Vec<String> = variant
            .sections
            .iter()
            .map(|s| {
                s.name
                    .clone()
                    .unwrap_or_else(|| s.option("name").unwrap().to_owned())
            })
            .collect();
        assert_eq!(names, ["loopback", "br-lan", "lan", "wan", "wan6"]);
        assert_eq!(
            variant.section("wan").unwrap().option("proto"),
            Some("dhcp")
        );
        assert_eq!(
            variant
                .section("lan")
                .unwrap()
                .get("ipaddr")
                .unwrap()
                .items(),
            ["10.0.0.1/24"]
        );
        // Only the WAN differs between the variant and the rendering.
        let diff = uniwrt_uci::diff(&variant, &rendered);
        let touched: Vec<_> = diff
            .sections
            .iter()
            .map(|s| s.after.as_ref().or(s.before.as_ref()).unwrap().name.clone())
            .collect();
        assert_eq!(
            touched,
            [
                None,
                Some("wan".to_owned()),
                Some("wan6".to_owned()),
                Some("wanb".to_owned())
            ]
        );
        // The same WAN on both sides: nothing to replace.
        let same = without_wan(&current, &current);
        assert!(uniwrt_uci::diff(&same, &current).is_empty());
    }
}
