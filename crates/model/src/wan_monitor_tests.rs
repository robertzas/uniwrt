use std::net::IpAddr;

use serde_json::json;
use uniwrt_core::Timestamp;

use super::{
    DnsVia, Probe, ProbeResult, ProbeRound, ProbeTarget, WanFault, WanHealth, WanMonitorSettings,
    assess,
};

fn result(key: &str, sent: u32, rtt_ms: &[f64]) -> ProbeResult {
    ProbeResult {
        key: key.to_owned(),
        sent,
        rtt_ms: rtt_ms.to_vec(),
        error: None,
    }
}

fn round(route: bool, results: Vec<ProbeResult>) -> ProbeRound {
    ProbeRound {
        at: Timestamp::from_unix_ms(1_000),
        route,
        first_hop: route.then(|| "10.0.0.1".parse::<IpAddr>().unwrap()),
        results,
    }
}

fn icmp(key: &str, answered: bool) -> ProbeResult {
    result(key, 10, if answered { &[5.0, 7.0] } else { &[] })
}

#[test]
fn the_defaults_are_valid_and_serialize_as_tagged_probes() {
    let settings = WanMonitorSettings::default();
    assert_eq!(settings.validate(), Ok(()));
    let value = serde_json::to_value(&settings).unwrap();
    assert_eq!(
        value["targets"][0]["probe"],
        json!({ "kind": "icmp", "host": null })
    );
    assert_eq!(
        value["targets"][5]["probe"],
        json!({ "kind": "dns", "via": { "via": "upstreams" }, "name": "openwrt.org" })
    );
    let back: WanMonitorSettings = serde_json::from_value(value).unwrap();
    assert_eq!(back, settings);
}

#[test]
fn targets_are_checked() {
    let with = |probe: Probe| WanMonitorSettings {
        enabled: true,
        targets: vec![ProbeTarget {
            id: "t".to_owned(),
            name: "T".to_owned(),
            probe,
        }],
    };
    let host = |host: &str| Probe::Icmp {
        host: Some(host.to_owned()),
    };
    let url = |url: &str| Probe::Http {
        url: url.to_owned(),
    };
    for good in [
        host("1.1.1.1"),
        host("2606:4700::1111"),
        host("one.one.one.one"),
        url("http://example.com/generate_204"),
        url("https://example.com:8443/x?y"),
        url("http://[2606:4700::1111]/"),
        Probe::Dns {
            via: DnsVia::Server {
                address: "9.9.9.9".parse().unwrap(),
            },
            name: "openwrt.org".to_owned(),
        },
    ] {
        assert_eq!(with(good.clone()).validate(), Ok(()), "{good:?}");
    }
    for bad in [
        host("1.1.1"),
        host("-x.example"),
        host("a b"),
        url("ftp://example.com"),
        url("http://exa mple.com"),
        url("http://"),
        Probe::Dns {
            via: DnsVia::Gateway,
            name: "not a name".to_owned(),
        },
        Probe::Dns {
            via: DnsVia::Server {
                address: "0.0.0.0".parse().unwrap(),
            },
            name: "openwrt.org".to_owned(),
        },
    ] {
        assert!(with(bad.clone()).validate().is_err(), "{bad:?}");
    }
    let mut settings = WanMonitorSettings::default();
    settings.targets[1].id = "first-hop".to_owned();
    assert!(settings.validate().unwrap_err().contains("Two targets"));
    settings.targets[1].id = "Upper".to_owned();
    assert!(settings.validate().is_err());
    settings.targets[1].id = "ok".to_owned();
    settings.targets[1].name = " ".to_owned();
    assert!(settings.validate().is_err());
    let mut many = WanMonitorSettings::default();
    for i in 0..10 {
        let mut target = many.targets[1].clone();
        target.id = format!("extra-{i}");
        many.targets.push(target);
    }
    assert!(many.validate().unwrap_err().contains("At most"));
}

#[test]
fn bands_and_loss_come_from_the_answers() {
    let r = result("x", 10, &[9.0, 3.0, 5.0, 4.0]);
    assert_eq!(r.band(), Some((3.0, 4.5, 9.0)));
    assert!((r.loss_percent() - 60.0).abs() < f64::EPSILON);
    assert_eq!(result("x", 1, &[7.0]).band(), Some((7.0, 7.0, 7.0)));
    assert_eq!(result("x", 10, &[]).band(), None);
    assert!((result("x", 0, &[]).loss_percent() - 100.0).abs() < f64::EPSILON);
}

#[test]
fn the_round_places_an_outage() {
    let settings = WanMonitorSettings::default();
    let healthy = || {
        vec![
            icmp("first-hop", true),
            icmp("cloudflare", true),
            icmp("google", false),
            icmp("quad9", false),
            result("resolver", 1, &[12.0]),
            result("isp-dns@10.0.0.53", 1, &[]),
            result("web", 1, &[30.0]),
        ]
    };
    assert_eq!(assess(&settings, &round(true, healthy())), WanHealth::Up);

    let mut no_dns = healthy();
    no_dns[4] = result("resolver", 1, &[]);
    assert_eq!(
        assess(&settings, &round(true, no_dns)),
        WanHealth::Down(WanFault::Dns)
    );

    let internet_lost = |first_hop: bool| {
        vec![
            icmp("first-hop", first_hop),
            icmp("cloudflare", false),
            icmp("google", false),
            icmp("quad9", false),
            result("web", 1, &[]),
        ]
    };
    assert_eq!(
        assess(&settings, &round(true, internet_lost(true))),
        WanHealth::Down(WanFault::Isp)
    );
    assert_eq!(
        assess(&settings, &round(true, internet_lost(false))),
        WanHealth::Down(WanFault::Line)
    );
    assert_eq!(
        assess(&settings, &round(false, internet_lost(false))),
        WanHealth::Down(WanFault::Gateway)
    );
    // Without a first-hop result, nothing places it.
    let mut unplaced = internet_lost(true);
    unplaced.remove(0);
    assert_eq!(
        assess(&settings, &round(true, unplaced)),
        WanHealth::Down(WanFault::Unknown)
    );
    // Only the first hop and DNS: nothing tells of the internet.
    assert_eq!(
        assess(
            &settings,
            &round(
                true,
                vec![icmp("first-hop", true), result("resolver", 1, &[3.0])]
            )
        ),
        WanHealth::Unknown
    );
}

#[test]
fn a_health_serializes_with_its_fault() {
    assert_eq!(
        serde_json::to_value(WanHealth::Down(WanFault::Isp)).unwrap(),
        json!({ "state": "down", "fault": "isp" })
    );
    assert_eq!(
        serde_json::to_value(WanHealth::Up).unwrap(),
        json!({ "state": "up" })
    );
}
