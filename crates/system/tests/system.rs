//! Rendered configs on OpenWrt (TESTING.md, System), on scripts/test-system.sh's topology:
//! `isp ═ gw ═ ap`. Each test renders a site for the gateway (and the AP) from their facts,
//! applies it and checks what uci, fw4 and netifd make of it.

use std::time::Duration;

use uniwrt_model::node::NodeRole;
use uniwrt_system::{
    Node, has_ipv4, ipv4, needs, render_for,
    site::{self, AP, GATEWAY, ISP_NET, ISP_PPPOE, ISP_VLAN_NET, Uplink},
};

const UP: Duration = Duration::from_secs(45);

fn gw() -> Node {
    Node::new("uniwrt-system-gw")
}

fn ap() -> Node {
    Node::new("uniwrt-system-ap")
}

fn in_subnet(status: &serde_json::Value, prefix: &str) -> bool {
    has_ipv4(status) && ipv4(status).iter().any(|a| a.starts_with(prefix))
}

/// Renders `site` for the gateway and applies it; with nftables, checks that fw4 accepts the
/// firewall and loads it.
fn gateway(site: &uniwrt_model::site::SiteConfig) -> Node {
    let gw = gw();
    let rendered = render_for(site, &gw, GATEWAY, NodeRole::Gateway);
    gw.apply(&rendered);
    if needs("nftables") {
        let check = gw.run("fw4 check", &[], &[]);
        // fw4 notes the includes packages bring (miniupnpd's chains); anything else is a
        // complaint.
        let complaints = check
            .stderr
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with("[!] Automatically including "))
            .count();
        assert!(
            check.ok && complaints == 0,
            "fw4 check: {}{}",
            check.stdout,
            check.stderr
        );
        gw.sh("nft list table inet fw4 >/dev/null", &[]);
    }
    gw
}

/// Renders `site` for the AP, applies it, and waits for its management address from the
/// gateway's DHCP server.
fn access_point(site: &uniwrt_model::site::SiteConfig) -> Node {
    let ap = ap();
    let rendered = render_for(site, &ap, AP, NodeRole::Ap);
    assert_eq!(rendered.services.get("dnsmasq"), Some(&false));
    ap.apply(&rendered);
    ap.wait_interface("lan", UP, |s| in_subnet(s, "10.0.0."));
    ap
}

/// The AP reaches the ISP through the gateway: forwarding and masquerading work.
fn reaches_isp(ap: &Node, address: &str) {
    if needs("nftables") {
        ap.sh(r#"ping -c 3 -W 2 "$1" >/dev/null"#, &[address]);
    }
}

#[test]
#[ignore = "needs scripts/test-system.sh's topology"]
fn defaults_behind_dhcp() {
    let site = site::defaults(Uplink::Dhcp);
    let gw = gateway(&site);
    gw.wait_interface("wan", UP, |s| in_subnet(s, ISP_NET));
    // Without VLANs, br-lan is the plain bridge OpenWrt ships.
    gw.wait_interface("lan", UP, |s| {
        ipv4(s) == ["10.0.0.1/24"] && s["l3_device"] == "br-lan"
    });
    let ap = access_point(&site);
    reaches_isp(&ap, "192.0.2.1");
}

#[test]
#[ignore = "needs scripts/test-system.sh's topology"]
fn home_behind_vlan_201() {
    if !needs("8021q") {
        return;
    }
    let site = site::home(Uplink::DhcpVlan);
    let gw = gateway(&site);
    gw.wait_interface("wan", UP, |s| {
        in_subnet(s, ISP_VLAN_NET) && s["l3_device"] == "eth1.201"
    });
    for (network, vid, address) in [
        ("lan", 1, "10.0.0.1/24"),
        ("iot", 30, "10.0.30.1/24"),
        ("guest", 40, "10.0.40.1/24"),
        ("securitycams", 50, "10.0.50.1/24"),
    ] {
        gw.wait_interface(network, UP, |s| {
            ipv4(s) == [address] && s["l3_device"] == format!("br-lan.{vid}").as_str()
        });
    }
    let trunk = ["1PU", "30", "40", "50"].map(str::to_owned).to_vec();
    assert_eq!(gw.bridge_vlans().get("eth0"), Some(&trunk));
    if needs("nftables") {
        // fw4 takes the 12-character zone name whole.
        let rules = gw.sh("nft list table inet fw4", &[]);
        assert!(
            rules.contains(r#"iifname "br-lan.50" jump input_securitycams"#),
            "{rules}"
        );
    }

    // dnsmasq's init script takes the LAN's options and network boot (the AP's lease below shows
    // it serves with them).
    let conf = gw.sh("cat /var/etc/dnsmasq.conf.*", &[]);
    for line in [
        "dhcp-option=lan,6,10.0.0.1",
        "dhcp-option=lan,42,10.0.0.1",
        "dhcp-option=lan,15,home.arpa",
        "dhcp-option=lan,26,1400",
        "dhcp-boot=tag:lan,netboot.xyz.efi,nas,10.0.0.40",
    ] {
        assert!(conf.lines().any(|l| l == line), "{line} in:\n{conf}");
    }

    let ap = access_point(&site);
    assert_eq!(ap.bridge_vlans().get("eth0"), Some(&trunk));
    for network in ["iot", "guest", "securitycams"] {
        ap.wait_interface(network, UP, |s| s["up"] == true);
    }
    reaches_isp(&ap, "198.51.100.1");
}

#[test]
#[ignore = "needs scripts/test-system.sh's topology"]
fn static_uplink() {
    let gw = gateway(&site::defaults(Uplink::Static));
    gw.wait_interface("wan", UP, |s| ipv4(s) == ["192.0.2.2/24"]);
    gw.sh("ping -c 3 -W 2 192.0.2.1 >/dev/null", &[]);
}

#[test]
#[ignore = "needs scripts/test-system.sh's topology"]
fn pppoe_uplink() {
    if !needs("pppoe") {
        return;
    }
    let gw = gateway(&site::defaults(Uplink::Pppoe));
    gw.wait_interface("wan", UP, |s| {
        in_subnet(s, ISP_PPPOE) && s["l3_device"] == "pppoe-wan"
    });
    gw.sh("ping -c 3 -W 2 203.0.113.1 >/dev/null", &[]);
}

#[test]
#[ignore = "needs scripts/test-system.sh's topology"]
fn sqm_shapes_the_wan_and_mwan3_reads_its_config() {
    use uniwrt_model::site::gateway::{BackupLink, BackupProto, Failover, Sqm};
    let mut site = site::defaults(Uplink::Static);
    site.wan.sqm = Some(Sqm {
        download_kbps: 50_000,
        upload_kbps: 10_000,
        overhead: Some(38),
        autotune: false,
    });
    // A backup that isn't plugged in: mwan3 must still read the config and track the WAN.
    site.wan.failover = Some(Failover {
        link: BackupLink::Usb {
            device: "usb0".to_owned(),
        },
        proto: BackupProto::Dhcp,
    });
    let gw = gateway(&site);
    gw.wait_interface("wan", UP, |s| ipv4(s) == ["192.0.2.2/24"]);
    let tracked = gw.sh("mwan3 interfaces 2>&1 || true", &[]);
    assert!(tracked.contains("interface wan is"), "{tracked}");
    assert!(tracked.contains("interface wanb is"), "{tracked}");
    if !needs("sch_cake") || !needs("ifb") {
        return;
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        let qdisc = gw.sh("tc qdisc show dev eth1", &[]);
        if qdisc.contains("cake") && qdisc.contains("bandwidth 10Mbit") {
            let ingress = gw.sh("tc qdisc show dev ifb4eth1", &[]);
            assert!(ingress.contains("bandwidth 50Mbit"), "{ingress}");
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "no cake on eth1: {qdisc}"
        );
        std::thread::sleep(Duration::from_secs(1));
    }
}

#[test]
#[ignore = "needs scripts/test-system.sh's topology"]
fn forwards_dmz_and_upnp() {
    use std::net::Ipv4Addr;

    use uniwrt_model::site::firewall::{ForwardProto, PortForward, PortRange};
    let mut site = site::defaults(Uplink::Static);
    site.firewall.forwards = vec![PortForward {
        name: "NAS".to_owned(),
        enabled: true,
        proto: ForwardProto::TcpUdp,
        ports: PortRange::one(8443),
        to: Ipv4Addr::new(10, 0, 0, 40),
        to_port: Some(443),
        source: None,
        reflection: true,
    }];
    site.firewall.dmz = Some(Ipv4Addr::new(10, 0, 0, 50));
    assert!(site.firewall.upnp.enabled, "a new site's default");
    let gw = gateway(&site);
    gw.wait_interface("wan", UP, |s| ipv4(s) == ["192.0.2.2/24"]);
    // miniupnpd's init script takes the package: secure mode, listening on the LAN.
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while !gw.run("pgrep miniupnpd", &[], &[]).ok {
        assert!(
            std::time::Instant::now() < deadline,
            "miniupnpd isn't running"
        );
        std::thread::sleep(Duration::from_secs(1));
    }
    let conf = gw.sh("cat /var/etc/miniupnpd.conf", &[]);
    for line in [
        "listening_ip=br-lan",
        "secure_mode=yes",
        "enable_natpmp=yes",
    ] {
        assert!(conf.lines().any(|l| l == line), "{line} in:\n{conf}");
    }
    // The agent reads its lease file with the DHCP leases.
    assert_eq!(gw.facts().port_mappings, []);
    if needs("nftables") {
        let rules = gw.sh("nft list table inet fw4", &[]);
        for dnat in ["dnat ip to 10.0.0.40:443", "dnat ip to 10.0.0.50"] {
            assert!(rules.contains(dnat), "{dnat} in:\n{rules}");
        }
        assert!(rules.contains("chain upnp_forward"), "{rules}");
    }
}
