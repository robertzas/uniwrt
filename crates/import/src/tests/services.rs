//! The gateway's services (D-132): what uniwrt rendered imports exactly, and a gateway set up
//! by hand (https-dns-proxy rewriting dnsmasq, ddns-scripts' services) maps onto the model.

use std::collections::BTreeMap;

use uniwrt_model::{
    node::NodeRole,
    site::{
        SiteConfig,
        services::{
            Ddns, DdnsProvider, DdnsSource, DohResolver, DotResolver, EncryptedResolver, Upstream,
        },
    },
};

use uniwrt_uci::{Package, Value};

use super::{
    Found,
    golden::round_trip,
    parse, run,
    support::{AP, GATEWAY, gateway_facts, home_site, secret, x86_facts},
};

fn entry(id: &str, hostname: &str, provider: DdnsProvider) -> Ddns {
    Ddns {
        id: id.to_owned(),
        enabled: true,
        hostname: hostname.to_owned(),
        provider,
        ipv6: false,
        source: DdnsSource::Wan,
        secret: Some(secret(5)),
    }
}

/// The home site with `upstream` and a DDNS name of each kind.
fn served(upstream: Upstream) -> SiteConfig {
    let mut site = home_site();
    site.services.upstream = upstream;
    let mut web6 = entry("duck", "me.duckdns.org", DdnsProvider::Duckdns);
    web6.ipv6 = true;
    web6.source = DdnsSource::Web;
    web6.enabled = false;
    site.services.ddns = vec![
        entry(
            "home",
            "home.example.com",
            DdnsProvider::Cloudflare {
                zone: "example.com".to_owned(),
            },
        ),
        web6,
        entry("desec", "me.dedyn.io", DdnsProvider::Desec),
        entry(
            "noip",
            "me.ddns.net",
            DdnsProvider::NoIp {
                username: "me".to_owned(),
            },
        ),
        entry(
            "dynu",
            "me.dynu.net",
            DdnsProvider::Dynu {
                username: "me".to_owned(),
            },
        ),
        entry("free", "me.mooo.com", DdnsProvider::Freedns),
        entry(
            "own",
            "home.example.org",
            DdnsProvider::Custom {
                url: "https://dyn.example/u?h=[DOMAIN]&ip=[IP]&p=[PASSWORD]".to_owned(),
                username: Some("me".to_owned()),
            },
        ),
    ];
    site.services.ntp_server = false;
    site
}

/// Sets dnsmasq's `noresolv` and `server`s, as https-dns-proxy or a hand edit would.
fn upstream(packages: &mut BTreeMap<String, Package>, servers: &[&str]) {
    let dhcp = packages.get_mut("dhcp").unwrap();
    let dnsmasq = dhcp
        .sections
        .iter_mut()
        .find(|s| s.kind == "dnsmasq")
        .unwrap();
    dnsmasq.set("noresolv", "1");
    dnsmasq.set(
        "server",
        Value::List(servers.iter().map(|s| (*s).to_owned()).collect()),
    );
}

/// The services of `site` with its secrets' names left out (import names its own).
fn services(site: &SiteConfig) -> (Upstream, Vec<Ddns>, bool) {
    let ddns = site
        .services
        .ddns
        .iter()
        .map(|d| Ddns {
            secret: None,
            ..d.clone()
        })
        .collect();
    (
        site.services.upstream.clone(),
        ddns,
        site.services.ntp_server,
    )
}

#[test]
fn doh_and_ddns_import_exactly() {
    let site = served(Upstream::strict(vec![
        EncryptedResolver::Doh(DohResolver {
            url: "https://cloudflare-dns.com/dns-query".to_owned(),
            bootstrap: vec!["1.1.1.1".parse().unwrap(), "1.0.0.1".parse().unwrap()],
        }),
        EncryptedResolver::Doh(DohResolver {
            url: "https://9.9.9.9/dns-query".to_owned(),
            bootstrap: Vec::new(),
        }),
    ]));
    let import = round_trip(
        &site,
        &[
            (GATEWAY, NodeRole::Gateway, gateway_facts()),
            (AP, NodeRole::Ap, gateway_facts()),
        ],
    );
    assert_eq!(services(&import.site), services(&site));
    // Each secret sealed under a name of its own, with its value.
    let home = import.site.services.ddns[0].secret.as_ref().unwrap();
    assert_eq!(import.secrets[home], "ddns-token");
}

#[test]
fn custom_servers_import_exactly() {
    let site = served(Upstream::custom(vec![
        "9.9.9.9".parse().unwrap(),
        "2620:fe::fe".parse().unwrap(),
    ]));
    let import = round_trip(&site, &[(GATEWAY, NodeRole::Gateway, gateway_facts())]);
    assert_eq!(services(&import.site), services(&site));
}

#[test]
fn the_layers_and_dot_import_exactly() {
    let dot = |name: &str, addresses: &[&str], port: u16, spki: &[&str]| {
        EncryptedResolver::Dot(DotResolver {
            name: name.to_owned(),
            addresses: addresses.iter().map(|ip| ip.parse().unwrap()).collect(),
            port,
            spki: spki.iter().map(|pin| (*pin).to_owned()).collect(),
        })
    };
    let layered = Upstream {
        encrypted: vec![
            dot("dns.quad9.net", &["9.9.9.9", "2620:fe::fe"], 853, &[]),
            dot(
                "dot.example.net",
                &["192.0.2.80"],
                8853,
                &["yioEpqeR4WtDwE9YxNVnCEkTxIjx6EEIwFSQW+lJsbc="],
            ),
            EncryptedResolver::Doh(DohResolver {
                url: "https://cloudflare-dns.com/dns-query".to_owned(),
                bootstrap: vec!["1.1.1.1".parse().unwrap(), "1.0.0.1".parse().unwrap()],
            }),
        ],
        servers: vec!["192.0.2.53".parse().unwrap()],
        isp: true,
        strict: false,
    };
    for upstream in [
        layered.clone(),
        Upstream {
            strict: true,
            servers: Vec::new(),
            isp: false,
            ..layered
        },
        Upstream {
            isp: true,
            ..Upstream::custom(vec!["192.0.2.53".parse().unwrap()])
        },
    ] {
        let site = served(upstream);
        let import = round_trip(&site, &[(GATEWAY, NodeRole::Gateway, gateway_facts())]);
        assert_eq!(services(&import.site), services(&site));
    }
}

#[test]
fn stock_stubby_and_unordered_isp_servers_map_as_far_as_the_model_holds() {
    let stock = (
        "stubby",
        "config stubby 'global'\n\toption round_robin_upstreams '1'\n\
         \tlist listen_address '127.0.0.1@5453'\n\tlist listen_address '0::1@5453'\n\n\
         config resolver\n\toption address '1.1.1.1'\n\toption tls_auth_name 'cloudflare-dns.com'\n\n\
         config resolver\n\toption address '1.0.0.1'\n\toption tls_auth_name 'cloudflare-dns.com'\n",
    );
    let import_with = |servers: &[&str], noresolv: bool, ordered: bool| {
        let mut packages = crate::release!("25.12");
        upstream(&mut packages, servers);
        let dnsmasq = packages
            .get_mut("dhcp")
            .unwrap()
            .sections
            .iter_mut()
            .find(|s| s.kind == "dnsmasq")
            .unwrap();
        if !noresolv {
            dnsmasq.remove("noresolv");
        }
        if ordered {
            dnsmasq.set("strictorder", "1");
        }
        packages.extend(parse(&[stock]));
        let found = [Found {
            node: GATEWAY,
            role: NodeRole::Gateway,
            facts: x86_facts(),
            packages,
        }];
        run(&SiteConfig::defaults(None), &found)
            .site
            .services
            .upstream
    };
    let cloudflare = EncryptedResolver::Dot(DotResolver {
        name: "cloudflare-dns.com".to_owned(),
        addresses: vec!["1.1.1.1".parse().unwrap(), "1.0.0.1".parse().unwrap()],
        port: 853,
        spki: Vec::new(),
    });
    assert_eq!(
        import_with(&["127.0.0.1#5453"], true, false),
        Upstream::strict(vec![cloudflare.clone()])
    );
    assert_eq!(
        import_with(&["127.0.0.1#5453", "9.9.9.9"], false, true),
        Upstream {
            encrypted: vec![cloudflare],
            servers: vec!["9.9.9.9".parse().unwrap()],
            isp: true,
            strict: false,
        }
    );
    // Without strictorder dnsmasq asks the WAN's servers as it likes; a proxy after a plain
    // server, or a port nothing listens on, isn't an order the model holds.
    for (servers, noresolv, ordered) in [
        (&["127.0.0.1#5453"][..], false, false),
        (&["9.9.9.9", "127.0.0.1#5453"][..], true, false),
        (&["127.0.0.1#5999"][..], true, false),
    ] {
        assert_eq!(
            import_with(servers, noresolv, ordered),
            Upstream::default(),
            "{servers:?}"
        );
    }
}

#[test]
fn a_hand_made_gateway_maps_what_the_model_holds() {
    let mut packages = crate::release!("25.12");
    upstream(&mut packages, &["127.0.0.1#5053", "127.0.0.1#5054"]);
    packages.extend(parse(&[
        (
            "https-dns-proxy",
            "config main 'config'\n\toption dnsmasq_config_update '*'\n\n\
             config https-dns-proxy\n\toption bootstrap_dns '8.8.8.8,8.8.4.4'\n\
             \toption resolver_url 'https://dns.google/dns-query'\n\n\
             config https-dns-proxy\n\toption bootstrap_dns '1.1.1.1'\n\
             \toption resolver_url 'https://cloudflare-dns.com/dns-query'\n",
        ),
        (
            "ddns",
            "config ddns 'global'\n\toption ddns_loglines '250'\n\n\
             config service 'myddns_ipv4'\n\toption service_name 'dyndns.org'\n\
             \toption lookup_host 'a.example.com'\n\n\
             config service 'cf'\n\toption enabled '1'\n\toption service_name 'cloudflare.com-v4'\n\
             \toption lookup_host 'Home.example.com'\n\toption domain 'home@example.com'\n\
             \toption username 'Bearer'\n\toption password 'tok'\n\n\
             config service 'cf_key'\n\toption service_name 'cloudflare.com-v4'\n\
             \toption lookup_host 'b.example.com'\n\toption domain 'b@example.com'\n\
             \toption username 'me@example.com'\n\n\
             config service 'scripted'\n\toption update_url 'https://x/[IP]'\n\
             \toption lookup_host 'c.example.com'\n\toption ip_source 'script'\n",
        ),
    ]));
    let found = [Found {
        node: GATEWAY,
        role: NodeRole::Gateway,
        facts: x86_facts(),
        packages,
    }];
    let import = run(&SiteConfig::defaults(None), &found);
    assert_eq!(
        import.site.services.upstream,
        Upstream::strict(vec![
            EncryptedResolver::Doh(DohResolver {
                url: "https://dns.google/dns-query".to_owned(),
                bootstrap: vec!["8.8.8.8".parse().unwrap(), "8.8.4.4".parse().unwrap()],
            }),
            EncryptedResolver::Doh(DohResolver {
                url: "https://cloudflare-dns.com/dns-query".to_owned(),
                bootstrap: vec!["1.1.1.1".parse().unwrap()],
            }),
        ])
    );
    // Another service, a Global API key and a script source stay raw.
    let ddns = &import.site.services.ddns;
    assert_eq!(ddns.len(), 1);
    assert_eq!(ddns[0].id, "cf");
    assert_eq!(ddns[0].hostname, "home.example.com");
    assert!(ddns[0].enabled);
    assert_eq!(import.secrets[ddns[0].secret.as_ref().unwrap()], "tok");
    assert_eq!(import.nodes[0].issues, Vec::new());
    let raw: Vec<_> = import.site.nodes[0]
        .raw
        .iter()
        .filter(|r| r.package == "ddns")
        .filter_map(|r| r.name.clone())
        .collect();
    assert_eq!(raw, ["myddns_ipv4", "cf_key", "scripted"]);
}

#[test]
fn a_conditional_server_keeps_the_isp_s_upstream() {
    let mut packages = crate::release!("25.12");
    upstream(&mut packages, &["/corp.example/10.1.1.1", "9.9.9.9"]);
    let found = [Found {
        node: GATEWAY,
        role: NodeRole::Gateway,
        facts: x86_facts(),
        packages,
    }];
    let import = run(&SiteConfig::defaults(None), &found);
    assert_eq!(import.site.services.upstream, Upstream::default());
}

#[test]
fn dns_filtering_and_its_bypass_import_exactly() {
    let mut site = served(Upstream::default());
    site.services.blocking.enabled = true;
    site.services.blocking.bypass = vec![
        "02:00:00:00:10:01".parse().unwrap(),
        "02:00:00:00:10:02".parse().unwrap(),
    ];
    let import = round_trip(&site, &[(GATEWAY, NodeRole::Gateway, gateway_facts())]);
    assert_eq!(import.site.services.blocking, site.services.blocking);
    let gateway = &import.nodes[0];
    assert!(
        gateway
            .residual
            .iter()
            .all(|d| !matches!(d.package.as_str(), "dhcp" | "firewall" | "system")),
        "{:?}",
        gateway.residual
    );
    // Every section of theirs maps onto the model, none kept raw.
    assert!(
        gateway
            .sections
            .iter()
            .filter(|s| s.package == "dhcp" || s.package == "firewall")
            .all(|s| matches!(&s.fate, crate::Fate::Mapped { kept } if kept.is_empty())),
        "{:?}",
        gateway.sections
    );
    // Without the bypass, filtering alone; without statistics, no query log.
    site.services.blocking.bypass.clear();
    let import = round_trip(&site, &[(GATEWAY, NodeRole::Gateway, gateway_facts())]);
    assert_eq!(import.site.services.blocking, site.services.blocking);
    site.services.blocking.stats = false;
    let import = round_trip(&site, &[(GATEWAY, NodeRole::Gateway, gateway_facts())]);
    assert_eq!(import.site.services.blocking, site.services.blocking);
    assert!(
        import.nodes[0]
            .residual
            .iter()
            .all(|d| d.package != "system")
    );
}
