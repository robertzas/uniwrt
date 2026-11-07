//! Port forwards, the DMZ host and UPnP from a stock 25.12 gateway with redirects as LuCI writes
//! them (with a DNAT target), some the model can't hold, and miniupnpd's package.

use std::net::Ipv4Addr;

use uniwrt_model::{
    node::NodeRole,
    site::{
        SiteConfig,
        firewall::{ForwardProto, PortRange},
    },
};

use super::{
    Found, assert_lossless, parse, run,
    support::{GATEWAY, x86_facts},
};

const REDIRECTS: &str = "
config redirect
\toption name 'Plex'
\toption target 'DNAT'
\toption src 'wan'
\toption src_dport '32400'
\toption dest 'lan'
\toption dest_ip '192.168.1.20'
\tlist proto 'tcp'

config redirect
\toption name 'Games'
\toption target 'DNAT'
\toption src 'wan'
\toption src_dport '3074:3075'
\toption dest 'lan'
\toption dest_ip '192.168.1.21'
\toption dest_port '3074-3075'
\toption reflection '0'
\toption enabled '0'

config redirect
\toption src 'wan'
\toption src_dport '22001'
\toption dest 'lan'
\toption dest_ip '192.168.1.22'
\toption dest_port '22'
\toption proto 'tcp'

config redirect
\toption name 'Shrunk'
\toption src 'wan'
\toption src_dport '1000-1010'
\toption dest 'lan'
\toption dest_ip '192.168.1.23'
\toption dest_port '2000'

config redirect
\toption name 'DMZ'
\toption target 'DNAT'
\toption src 'wan'
\toption proto 'all'
\toption dest 'lan'
\toption dest_ip '192.168.1.50'
";

/// OpenWrt's `upnpd`, as miniupnpd ships it and its init script leaves it (with a uuid).
fn upnpd(enabled: &str) -> String {
    format!(
        "config upnpd 'config'
\toption enabled '{enabled}'
\toption enable_natpmp '1'
\toption enable_upnp '1'
\toption secure_mode '1'
\toption log_output '0'
\toption download '1024'
\toption upload '512'
\toption internal_iface 'lan'
\toption port '5000'
\toption upnp_lease_file '/var/run/miniupnpd.leases'
\toption igdv1 '1'
\toption uuid '0b5e2c1a-3f4d-4e5f-8a9b-0c1d2e3f4a5b'

config perm_rule
\toption action 'allow'
\toption ext_ports '1024-65535'
\toption int_addr '0.0.0.0/0'
\toption int_ports '1024-65535'
\toption comment 'Allow high ports'

config perm_rule
\toption action 'deny'
\toption ext_ports '0-65535'
\toption int_addr '0.0.0.0/0'
\toption int_ports '0-65535'
\toption comment 'Default deny'
"
    )
}

fn gateway(upnpd: Option<String>) -> crate::Import {
    let mut packages = crate::release!("25.12");
    let firewall = format!(
        "{}{REDIRECTS}",
        include_str!("../../../uci/fixtures/openwrt/25.12/firewall")
    );
    packages.extend(parse(&[("firewall", &firewall)]));
    if let Some(upnpd) = upnpd {
        packages.extend(parse(&[("upnpd", &upnpd)]));
    }
    let found = [Found {
        node: GATEWAY,
        role: NodeRole::Gateway,
        facts: x86_facts(),
        packages,
    }];
    let import = run(&SiteConfig::defaults(None), &found);
    assert_lossless(&import);
    import
}

#[test]
fn named_redirects_become_forwards_and_the_dmz() {
    let import = gateway(None);
    let firewall = &import.site.firewall;
    let forwards: Vec<_> = firewall
        .forwards
        .iter()
        .map(|f| {
            (
                f.name.as_str(),
                f.proto,
                f.ports,
                f.to,
                f.to_port,
                f.enabled,
                f.reflection,
            )
        })
        .collect();
    assert_eq!(
        forwards,
        [
            (
                "Plex",
                ForwardProto::Tcp,
                PortRange::one(32400),
                Ipv4Addr::new(192, 168, 1, 20),
                None,
                true,
                true
            ),
            (
                "Games",
                ForwardProto::TcpUdp,
                PortRange {
                    first: 3074,
                    last: 3075
                },
                Ipv4Addr::new(192, 168, 1, 21),
                None,
                false,
                false
            ),
        ]
    );
    assert_eq!(firewall.dmz, Some(Ipv4Addr::new(192, 168, 1, 50)));
    // The unnamed redirect and the one whose ranges differ stay as they are.
    let node = &import.nodes[0];
    let raw = node
        .sections
        .iter()
        .filter(|s| s.package == "firewall" && s.kind == "redirect" && s.fate == crate::Fate::Raw)
        .count();
    assert_eq!(raw, 2, "{:?}", node.sections);
    // Without miniupnpd, UPnP is off (and renders nothing).
    assert!(!firewall.upnp.enabled);
    assert_eq!(firewall.upnp.networks, ["lan"]);
}

#[test]
fn miniupnpds_package_becomes_upnp() {
    for (enabled, on) in [("0", false), ("1", true)] {
        let import = gateway(Some(upnpd(enabled)));
        let upnp = &import.site.firewall.upnp;
        assert_eq!(upnp.enabled, on);
        assert_eq!(upnp.networks, ["lan"]);
        // Rendered as the node has it, its uuid carried over.
        let residual = &import.nodes[0].residual;
        assert!(
            residual.iter().all(|d| d.package != "upnpd"),
            "{residual:?}"
        );
    }
}
