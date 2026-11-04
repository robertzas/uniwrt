//! The board's Ethernet layout from `/etc/board.json` (part of the `board` set): its `network`
//! roles, its swconfig `switch` and `system.compat_version`, as OpenWrt's `config_generate`
//! reads them.

use std::collections::BTreeMap;

use serde::Deserialize;
use uniwrt_model::facts::{BoardLayout, CpuPort, SwitchLayout};

#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct Json {
    network: BTreeMap<String, Role>,
    switch: BTreeMap<String, Switch>,
    pub(super) system: System,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct System {
    /// The MAC printed on the label, which the board set keeps.
    pub(super) label_macaddr: Option<String>,
    compat_version: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Role {
    ports: Vec<String>,
    device: Option<String>,
    macaddr: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Switch {
    ports: Vec<SwitchPort>,
    roles: Vec<SwitchRole>,
}

#[derive(Deserialize)]
struct SwitchPort {
    num: u8,
    #[serde(default)]
    role: Option<String>,
    /// Set on CPU ports.
    #[serde(default)]
    device: Option<String>,
}

#[derive(Deserialize)]
struct SwitchRole {
    role: String,
    /// Such as `1 2 3 4 0t`.
    #[serde(default)]
    ports: String,
}

pub(super) fn parse(json: Json) -> BoardLayout {
    let role = |name: &str| json.network.get(name);
    let mac = |name: &str| {
        role(name)
            .and_then(|r| r.macaddr.as_deref())
            .and_then(|mac| mac.parse().ok())
    };
    let lan = role("lan").map_or_else(Vec::new, |r| {
        if r.ports.is_empty() {
            r.device.iter().cloned().collect()
        } else {
            r.ports.clone()
        }
    });
    let wan = role("wan").and_then(|r| r.device.clone().or_else(|| r.ports.first().cloned()));
    BoardLayout {
        lan,
        lan_mac: mac("lan"),
        wan,
        wan_mac: mac("wan"),
        // board.json names one switch on every board that has one.
        switch: json
            .switch
            .into_iter()
            .next()
            .map(|(name, switch)| switch_layout(name, &switch)),
        compat_version: json.system.compat_version,
    }
}

fn switch_layout(name: String, switch: &Switch) -> SwitchLayout {
    let user = |role: &str| -> Vec<u8> {
        switch
            .ports
            .iter()
            .filter(|p| p.device.is_none() && p.role.as_deref() == Some(role))
            .map(|p| p.num)
            .collect()
    };
    let cpu = |num: u8| {
        switch.ports.iter().find_map(|p| {
            (p.num == num)
                .then(|| p.device.clone())
                .flatten()
                .map(|device| CpuPort { num, device })
        })
    };
    // A role's CPU port is the tagged member of its port list that has a device; without a
    // role, the first CPU port.
    let role_cpu = |role: &str| {
        switch
            .roles
            .iter()
            .filter(|r| r.role == role)
            .flat_map(|r| r.ports.split_whitespace())
            .filter_map(|token| token.strip_suffix('t')?.parse().ok())
            .find_map(cpu)
    };
    let first_cpu = switch.ports.iter().find_map(|p| cpu(p.num));
    let wan_ports = user("wan");
    SwitchLayout {
        name,
        lan_ports: user("lan"),
        lan_cpu: role_cpu("lan").or_else(|| first_cpu.clone()),
        wan_cpu: if wan_ports.is_empty() {
            None
        } else {
            role_cpu("wan").or(first_cpu)
        },
        wan_ports,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{from_value, json};
    use uniwrt_model::facts::CpuPort;

    use super::parse;

    #[test]
    fn dsa_ports_and_macs() {
        let layout = parse(
            from_value(json!({
                "network": {
                    "lan": {"ports": ["lan1", "lan2"], "protocol": "static", "macaddr": "94:83:c4:00:00:11"},
                    "wan": {"device": "wan", "protocol": "dhcp", "macaddr": "bogus"}
                },
                "system": {"compat_version": "1.1"}
            }))
            .unwrap(),
        );
        assert_eq!(layout.lan, ["lan1", "lan2"]);
        assert_eq!(layout.lan_mac, "94:83:c4:00:00:11".parse().ok());
        assert_eq!(layout.wan.as_deref(), Some("wan"));
        assert_eq!(layout.wan_mac, None);
        assert_eq!(layout.compat_version.as_deref(), Some("1.1"));
        assert_eq!(layout.switch, None);
    }

    #[test]
    fn a_single_nic_has_a_lan_device_and_no_wan() {
        let layout = parse(from_value(json!({"network": {"lan": {"device": "eth0"}}})).unwrap());
        assert_eq!(layout.lan, ["eth0"]);
        assert_eq!(layout.wan, None);
    }

    #[test]
    fn swconfig_roles_name_their_cpu_ports() {
        let layout = parse(
            from_value(json!({
                "network": {"lan": {"device": "eth0.1"}, "wan": {"device": "eth1.2"}},
                "switch": {"switch0": {
                    "enable": true, "reset": true,
                    "ports": [
                        {"num": 0, "device": "eth0", "need_tag": false, "want_untag": true},
                        {"num": 1, "role": "lan", "index": 4},
                        {"num": 2, "role": "lan", "index": 3},
                        {"num": 5, "role": "wan"},
                        {"num": 6, "device": "eth1", "need_tag": false}
                    ],
                    "roles": [
                        {"role": "lan", "ports": "1 2 0t", "device": "eth0.1"},
                        {"role": "wan", "ports": "5 6t", "device": "eth1.2"}
                    ]
                }}
            }))
            .unwrap(),
        );
        let switch = layout.switch.unwrap();
        assert_eq!(switch.name, "switch0");
        assert_eq!(switch.lan_ports, [1, 2]);
        assert_eq!(switch.wan_ports, [5]);
        let cpu = |num, device: &str| {
            Some(CpuPort {
                num,
                device: device.to_owned(),
            })
        };
        assert_eq!(switch.lan_cpu, cpu(0, "eth0"));
        assert_eq!(switch.wan_cpu, cpu(6, "eth1"));
        assert_eq!(switch.cpu_ports().len(), 2);
        assert_eq!(layout.wan.as_deref(), Some("eth1.2"));
    }

    #[test]
    fn a_switch_without_roles_uses_its_first_cpu_port() {
        let layout = parse(
            from_value(json!({"switch": {"switch0": {"ports": [
                {"num": 1, "role": "lan"}, {"num": 0, "device": "eth0"}
            ]}}}))
            .unwrap(),
        );
        let switch = layout.switch.unwrap();
        assert_eq!(switch.lan_cpu.map(|p| p.num), Some(0));
        assert_eq!(switch.wan_cpu, None);
    }
}
