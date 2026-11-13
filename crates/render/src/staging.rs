//! The staging config of a factory-default device on the adoption port (features/adoption.md,
//! Path C; D-107): the changes ZTP's first-boot script makes, as packages for a transaction.
//! It makes an access point or a bridge safe on any LAN: the LAN bridge takes an address over
//! DHCP and serves none (dnsmasq and odhcpd off, as an access point's render has them, and their
//! LAN pool ignored besides), the WAN port joins the bridge so any port reaches the site, and
//! Wi-Fi is off until the node's own render brings it. Everything else stays as the device
//! shipped it.
//!
//! The DHCP servers stop before the network reloads (the transaction tool stops the services
//! that go off first), so the device's DHCP client never meets its own server: a lease from it
//! would be kept for hours.

use std::collections::BTreeMap;

use uniwrt_uci::{Package, Value};

use crate::Issue;

/// A staging config: the packages that change, and the services to switch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Staging {
    /// `network`, and `dhcp` and `wireless` where the device has them.
    pub packages: BTreeMap<String, Package>,
    /// dnsmasq and odhcpd, off.
    pub services: BTreeMap<&'static str, bool>,
}

/// The staging config of a device holding `current`, whose `network` must have a `lan`
/// interface.
pub fn staging(current: &BTreeMap<String, Package>) -> Result<Staging, Issue> {
    let mut staged = BTreeMap::new();
    let mut network = current
        .get("network")
        .cloned()
        .ok_or_else(|| Issue::new("network", "The device has no network configuration"))?;
    let lan = network
        .sections
        .iter_mut()
        .find(|s| s.kind == "interface" && s.name.as_deref() == Some("lan"))
        .ok_or_else(|| Issue::new("network/lan", "The device has no LAN interface"))?;
    lan.set("proto", "dhcp");
    for option in ["ipaddr", "netmask", "ip6assign", "gateway", "dns"] {
        lan.remove(option);
    }
    bridge_wan(&mut network);
    staged.insert("network".to_owned(), network);

    if let Some(mut dhcp) = current.get("dhcp").cloned() {
        if let Some(lan) = dhcp.section_mut("lan") {
            lan.set("ignore", "1");
            lan.set("dhcpv6", "disabled");
            lan.set("ra", "disabled");
        }
        if let Some(dnsmasq) = dhcp.sections.iter_mut().find(|s| s.kind == "dnsmasq") {
            dnsmasq.set("localuse", "0");
        }
        staged.insert("dhcp".to_owned(), dhcp);
    }

    if let Some(mut wireless) = current.get("wireless").cloned() {
        for radio in wireless
            .sections
            .iter_mut()
            .filter(|s| s.kind == "wifi-device")
        {
            radio.set("disabled", "1");
        }
        staged.insert("wireless".to_owned(), wireless);
    }
    Ok(Staging {
        packages: staged,
        services: BTreeMap::from([("dnsmasq", false), ("odhcpd", false)]),
    })
}

/// Adds the WAN's device to `br-lan`'s ports and drops `wan` and `wan6`, as the first-boot
/// script does: only for a plain device (not an alias, `@lan`) and a `br-lan` device section.
fn bridge_wan(network: &mut Package) {
    let Some(wan) = network
        .section("wan")
        .filter(|s| s.kind == "interface")
        .and_then(|s| s.option("device"))
        .filter(|device| !device.starts_with('@'))
        .map(str::to_owned)
    else {
        return;
    };
    let Some(bridge) = network
        .sections
        .iter_mut()
        .find(|s| s.kind == "device" && s.option("name") == Some("br-lan"))
    else {
        return;
    };
    let has = bridge
        .get("ports")
        .map(Value::items)
        .is_some_and(|ports| ports.contains(&wan));
    if !has {
        bridge.add_list("ports", wan);
    }
    network
        .sections
        .retain(|s| !(s.kind == "interface" && matches!(s.name.as_deref(), Some("wan" | "wan6"))));
}

#[cfg(test)]
mod tests {
    use super::*;

    const NETWORK: &str = "
config interface 'loopback'
	option device 'lo'
	option proto 'static'
	option ipaddr '127.0.0.1'
	option netmask '255.0.0.0'

config globals 'globals'
	option ula_prefix 'fd2e:1b0c:5a8b::/48'

config device
	option name 'br-lan'
	option type 'bridge'
	list ports 'lan1'
	list ports 'lan2'

config interface 'lan'
	option device 'br-lan'
	option proto 'static'
	option ipaddr '192.168.1.1'
	option netmask '255.255.255.0'
	option ip6assign '60'

config interface 'wan'
	option device 'wan'
	option proto 'dhcp'

config interface 'wan6'
	option device 'wan'
	option proto 'dhcpv6'
";

    const DHCP: &str = "
config dnsmasq
	option domainneeded '1'
	option localise_queries '1'

config dhcp 'lan'
	option interface 'lan'
	option start '100'
	option limit '150'
	option leasetime '12h'
	option dhcpv4 'server'
	option dhcpv6 'server'
	option ra 'server'

config dhcp 'wan'
	option interface 'wan'
	option ignore '1'
";

    const WIRELESS: &str = "
config wifi-device 'radio0'
	option type 'mac80211'
	option band '2g'
	option disabled '1'

config wifi-device 'radio1'
	option type 'mac80211'
	option band '5g'

config wifi-iface 'default_radio1'
	option device 'radio1'
	option network 'lan'
	option mode 'ap'
	option ssid 'OpenWrt'
	option encryption 'none'
";

    fn factory() -> BTreeMap<String, Package> {
        [("network", NETWORK), ("dhcp", DHCP), ("wireless", WIRELESS)]
            .into_iter()
            .map(|(name, text)| (name.to_owned(), uniwrt_uci::parse(name, text).unwrap()))
            .collect()
    }

    #[test]
    fn an_access_point_asks_for_an_address_serves_none_and_bridges_its_wan() {
        let staging = staging(&factory()).unwrap();
        assert_eq!(
            staging.services,
            BTreeMap::from([("dnsmasq", false), ("odhcpd", false)])
        );
        let staged = staging.packages;
        let network = &staged["network"];
        let lan = network.section("lan").unwrap();
        assert_eq!(lan.option("proto"), Some("dhcp"));
        assert_eq!(lan.option("device"), Some("br-lan"));
        for gone in ["ipaddr", "netmask", "ip6assign"] {
            assert_eq!(lan.get(gone), None, "{gone}");
        }
        assert!(network.section("wan").is_none());
        assert!(network.section("wan6").is_none());
        let bridge = network.sections_of("device").next().unwrap();
        assert_eq!(
            bridge.get("ports").unwrap().items(),
            ["lan1", "lan2", "wan"]
        );

        let dhcp = &staged["dhcp"];
        let lan = dhcp.section("lan").unwrap();
        assert_eq!(lan.option("ignore"), Some("1"));
        assert_eq!(lan.option("dhcpv6"), Some("disabled"));
        assert_eq!(lan.option("ra"), Some("disabled"));
        assert_eq!(
            dhcp.sections_of("dnsmasq")
                .next()
                .unwrap()
                .option("localuse"),
            Some("0")
        );

        let wireless = &staged["wireless"];
        assert!(
            wireless
                .sections_of("wifi-device")
                .all(|radio| radio.option("disabled") == Some("1"))
        );
        // The SSID stays as shipped: the radio carrying it is off.
        assert!(wireless.section("default_radio1").is_some());
        for package in staged.values() {
            package.validate().unwrap();
        }
    }

    #[test]
    fn a_device_without_wifi_has_no_wireless_and_a_bridged_wan_stays_once() {
        let mut current = factory();
        current.remove("wireless");
        let network = current.get_mut("network").unwrap();
        let bridge = network
            .sections
            .iter_mut()
            .find(|s| s.kind == "device")
            .unwrap();
        bridge.add_list("ports", "wan");
        let staged = staging(&current).unwrap().packages;
        let bridge = staged["network"].sections_of("device").next().unwrap();
        assert_eq!(
            bridge.get("ports").unwrap().items(),
            ["lan1", "lan2", "wan"]
        );
        assert!(!staged.contains_key("wireless"));
    }

    #[test]
    fn a_wan_on_an_alias_stays_and_a_device_without_a_lan_is_refused() {
        let mut current = factory();
        let network = current.get_mut("network").unwrap();
        network.section_mut("wan").unwrap().set("device", "@lan");
        let staged = staging(&current).unwrap().packages;
        assert!(staged["network"].section("wan").is_some());

        let network = current.get_mut("network").unwrap();
        network
            .sections
            .retain(|s| s.name.as_deref() != Some("lan"));
        let err = staging(&current).unwrap_err();
        assert_eq!(err.path, "network/lan");
    }
}
