//! The gateway's network services on OpenWrt (D-132), on scripts/test-system.sh's topology:
//! DoH through https-dns-proxy behind dnsmasq, and the DDNS updater, beside the `dhcp` and
//! `firewall` packages render owns; DNS filtering (D-133) with a client that bypasses it.

use std::time::{Duration, Instant};

use uniwrt_model::{
    node::NodeRole,
    site::{
        blocking::{BLOCKLIST_CONF, BLOCKLIST_FILE, CONF_DIR},
        node::ManagedPackage,
        services::{Ddns, DdnsProvider, DdnsSource, DohResolver, Upstream},
    },
};
use uniwrt_system::{
    Node, ipv4, needs, render_for,
    site::{self, AP, GATEWAY, Uplink, secret},
};

const UP: Duration = Duration::from_secs(45);

/// Waits until `check` holds, up to `within`.
fn eventually(within: Duration, what: &str, mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + within;
    while !check() {
        assert!(Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_secs(1));
    }
}

#[test]
#[ignore = "needs scripts/test-system.sh's topology"]
fn doh_and_ddns_run_beside_what_render_owns() {
    let mut site = site::defaults(Uplink::Static);
    site.services.upstream = Upstream::Doh {
        resolvers: vec![
            DohResolver {
                url: "https://cloudflare-dns.com/dns-query".to_owned(),
                bootstrap: vec!["1.1.1.1".parse().unwrap(), "1.0.0.1".parse().unwrap()],
            },
            DohResolver {
                url: "https://9.9.9.9/dns-query".to_owned(),
                bootstrap: Vec::new(),
            },
        ],
    };
    site.services.ddns = vec![Ddns {
        id: "lab".to_owned(),
        enabled: true,
        hostname: "gw.example.net".to_owned(),
        // The ISP answers no HTTP: the update fails, the updater keeps running.
        provider: DdnsProvider::Custom {
            url: "http://192.0.2.1/update?h=[DOMAIN]&ip=[IP]&p=[PASSWORD]".to_owned(),
            username: None,
        },
        ipv6: false,
        source: DdnsSource::Wan,
        secret: Some(secret(7)),
    }];
    let gw = Node::new("uniwrt-system-gw");
    let rendered = render_for(&site, &gw, GATEWAY, NodeRole::Gateway);
    assert_eq!(rendered.services.get("https-dns-proxy"), Some(&true));
    assert_eq!(rendered.services.get("ddns"), Some(&true));
    gw.apply(&rendered);
    gw.wait_interface("wan", UP, |s| ipv4(s) == ["192.0.2.2/24"]);

    // procd runs an instance per resolver, each on its port on the loopback.
    eventually(UP, "two https-dns-proxy instances", || {
        gw.facts().services.doh_instances == Some(2)
    });
    let udp = gw.sh("cat /proc/net/udp", &[]);
    for port in ["0100007F:13BD", "0100007F:13BE"] {
        assert!(udp.contains(port), "127.0.0.1 listens on {port}:\n{udp}");
    }
    // dnsmasq forwards to them alone, and resolves the NTP servers through the bootstrap.
    let conf = gw.sh("cat /var/etc/dnsmasq.conf.*", &[]);
    for line in [
        "no-resolv",
        "server=127.0.0.1#5053",
        "server=127.0.0.1#5054",
        "server=/0.openwrt.pool.ntp.org/1.1.1.1",
    ] {
        assert!(conf.lines().any(|l| l == line), "{line} in:\n{conf}");
    }
    // The proxy's init script left dnsmasq's and the firewall's packages as render wrote them.
    for package in [ManagedPackage::Dhcp, ManagedPackage::Firewall] {
        let name = package.name();
        let export = gw.sh(r#"uci export "$1""#, &[name]);
        let read = uniwrt_uci::parse_export(&export).unwrap();
        let diff = uniwrt_uci::diff(&rendered.packages[&package], &read[0]);
        assert!(diff.is_empty(), "{name} changed: {diff:?}");
    }

    // ddns-scripts runs the entry's updater, which the services set sees.
    eventually(UP, "the DDNS updater runs", || {
        let facts = gw.facts();
        facts
            .services
            .ddns
            .iter()
            .any(|d| d.id == "lab" && d.running)
    });
}

/// What `nslookup name` through the gateway answers on `node`, with its errors.
fn lookup(node: &Node, name: &str) -> String {
    let out = node.run("nslookup \"$1\" 10.0.0.1 2>&1", &[name], &[]);
    format!("{}{}", out.stdout, out.stderr)
}

#[test]
#[ignore = "needs scripts/test-system.sh's topology"]
fn dns_filtering_blocks_what_its_list_names_but_not_for_a_bypassed_client() {
    let mut site = site::defaults(Uplink::Static);
    site.services.blocking.enabled = true;
    // Another client bypasses at first: the unfiltered instance runs, the AP is filtered.
    site.services.blocking.bypass = vec!["02:00:00:00:99:01".parse().unwrap()];
    let gw = Node::new("uniwrt-system-gw");
    gw.apply(&render_for(&site, &gw, GATEWAY, NodeRole::Gateway));
    gw.wait_interface("wan", UP, |s| ipv4(s) == ["192.0.2.2/24"]);
    let ap = Node::new("uniwrt-system-ap");
    ap.apply(&render_for(&site, &ap, AP, NodeRole::Ap));
    ap.wait_interface("lan", UP, |s| {
        ipv4(s).iter().any(|a| a.starts_with("10.0.0."))
    });

    // The unfiltered instance answers DNS on 5354 (0x14EA) and serves no DHCP.
    eventually(UP, "the unfiltered instance listens", || {
        gw.sh("cat /proc/net/udp", &[]).contains(":14EA")
    });
    let conf = gw.sh("cat /var/etc/dnsmasq.conf.unfiltered", &[]);
    assert!(!conf.contains("dhcp-range"), "{conf}");
    assert!(conf.lines().any(|l| l == "port=5354"), "{conf}");

    // The controller's push: the list, then the conf file that loads it, then a restart.
    gw.sh(
        r#"mkdir -p "$1" && printf '# uniwrt test\nserver=/blocked.example/\n' > "$2" &&
           printf 'servers-file=%s\n' "$2" > "$3" && /etc/init.d/dnsmasq restart"#,
        &[CONF_DIR, BLOCKLIST_FILE, BLOCKLIST_CONF],
    );
    eventually(UP, "the list blocks", || {
        lookup(&ap, "blocked.example").contains("NXDOMAIN")
    });
    // A refresh is read again on SIGHUP (`reload`), without a restart.
    gw.sh(
        r#"printf 'server=/blocked.example/\nserver=/also.example/\n' > "$1" &&
           /etc/init.d/dnsmasq reload"#,
        &[BLOCKLIST_FILE],
    );
    eventually(UP, "the refreshed list blocks", || {
        lookup(&ap, "also.example").contains("NXDOMAIN")
    });

    // The AP bypasses: its DNS goes to the unfiltered instance, which still knows local names
    // through the main one (its DHCP leases' names, which only the main instance holds).
    let mac = ap.sh("cat /sys/class/net/br-lan/address", &[]);
    let lease = gw.sh(r#"grep -i " $1 " /tmp/dhcp.leases"#, &[mac.trim()]);
    let fields: Vec<&str> = lease.split_whitespace().collect();
    let (ip, name) = (fields[2].to_owned(), format!("{}.lan", fields[3]));
    site.services.blocking.bypass = vec![mac.trim().parse().unwrap()];
    let rendered = render_for(&site, &gw, GATEWAY, NodeRole::Gateway);
    gw.apply(&rendered);
    if needs("nftables") {
        let chain = gw.sh("nft list chain inet fw4 dstnat_lan", &[]);
        assert!(chain.contains("redirect to :5354"), "{chain}");
    }
    eventually(UP, "the bypassed client resolves local names", || {
        lookup(&ap, &name).contains(&ip)
    });
    let answer = lookup(&ap, "blocked.example");
    assert!(!answer.contains("NXDOMAIN"), "{answer}");
}
