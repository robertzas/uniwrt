//! The WAN a trial tries: the gateway's own `network` package with only its WAN swapped for
//! the rendered one, so nothing else changes while the test runs (D-075).

use uniwrt_model::facts::BoardLayout;
use uniwrt_uci::{Package, Section};

/// `current` with its WAN sections replaced by `rendered`'s: interfaces named `wan…` (`wan`,
/// `wan6`, `wan_6`, `wan6rd`), the devices on the WAN port (its own, an 802.1Q device on it),
/// and on a swconfig switch the VLANs of the WAN's ports.
pub(crate) fn patch(current: &Package, rendered: &Package, layout: &BoardLayout) -> Package {
    let mut patched = Package::new(&current.name);
    patched.sections = current
        .sections
        .iter()
        .filter(|section| !is_wan(section, layout))
        .cloned()
        .collect();
    patched.sections.extend(
        rendered
            .sections
            .iter()
            .filter(|section| is_wan(section, layout))
            .cloned(),
    );
    patched
}

fn is_wan(section: &Section, layout: &BoardLayout) -> bool {
    match section.kind.as_str() {
        "interface" => section
            .name
            .as_deref()
            .is_some_and(|n| n.starts_with("wan")),
        "device" => {
            let Some(port) = layout.wan.as_deref() else {
                return false;
            };
            let on_port = |name: &str| {
                name == port || name.strip_prefix(port).is_some_and(|r| r.starts_with('.'))
            };
            section.option("name").is_some_and(on_port)
                || (section.option("type") == Some("8021q")
                    && section.option("ifname") == Some(port))
        }
        "switch_vlan" => {
            let Some(switch) = &layout.switch else {
                return false;
            };
            let ports = match section.get("ports") {
                Some(uniwrt_uci::Value::Single(ports)) => ports.clone(),
                Some(uniwrt_uci::Value::List(ports)) => ports.join(" "),
                None => String::new(),
            };
            ports.split_whitespace().any(|p| {
                let num = p.trim_end_matches(['t', 'u', '*']);
                switch.wan_ports.iter().any(|w| w.to_string() == num)
            })
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use uniwrt_model::facts::BoardLayout;

    use super::patch;

    const CURRENT: &str = "config interface 'loopback'
\toption device 'lo'
\toption proto 'static'

config device
\toption name 'br-lan'
\toption type 'bridge'
\tlist ports 'lan1'

config device
\toption name 'wan'
\toption macaddr '02:00:00:00:00:09'

config interface 'lan'
\toption device 'br-lan'
\toption proto 'static'

config interface 'wan'
\toption device 'wan'
\toption proto 'dhcp'

config interface 'wan6rd'
\toption proto '6rd'
";

    const RENDERED: &str = "config interface 'lan'
\toption device 'br-lan.1'
\toption proto 'static'

config device
\toption type '8021q'
\toption ifname 'wan'
\toption vid '201'
\toption name 'wan.201'

config interface 'wan'
\toption device 'wan.201'
\toption proto 'dhcp'

config interface 'wan6'
\toption device 'wan.201'
\toption proto 'dhcpv6'
";

    #[test]
    fn only_the_wan_changes() {
        let current = uniwrt_uci::parse("network", CURRENT).unwrap();
        let rendered = uniwrt_uci::parse("network", RENDERED).unwrap();
        let layout = BoardLayout {
            wan: Some("wan".to_owned()),
            ..BoardLayout::default()
        };
        let patched = patch(&current, &rendered, &layout);
        let names: Vec<String> = patched
            .sections
            .iter()
            .map(|s| {
                s.name
                    .clone()
                    .or(s.option("name").map(str::to_owned))
                    .unwrap()
            })
            .collect();
        assert_eq!(
            names,
            ["loopback", "br-lan", "lan", "wan.201", "wan", "wan6"]
        );
        // The LAN stays as the node holds it, not as render would write it.
        assert_eq!(patched.sections[2].option("device"), Some("br-lan"));
    }
}
