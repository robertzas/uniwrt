//! The `services` set: ddns-scripts' run files per section (the registered address, the last
//! update in seconds of uptime, whether the updater runs) and https-dns-proxy's running
//! instances, with the node's clock and uptime, which place the update in time; and the DNS
//! blocklist's header line and conf file (D-133).

use std::net::IpAddr;

use serde::Deserialize;
use uniwrt_core::Timestamp;
use uniwrt_model::{
    blocklists::parse_header,
    facts::{BlocklistFacts, DdnsFacts, ServiceFacts},
};

#[derive(Deserialize)]
pub(super) struct Raw {
    #[serde(default)]
    now: Option<i64>,
    #[serde(default)]
    uptime: Option<i64>,
    #[serde(default)]
    ddns: Vec<Ddns>,
    #[serde(default)]
    doh: Option<u32>,
    /// The blocklist's first line.
    #[serde(default)]
    blocklist: Option<String>,
    #[serde(default)]
    blocklist_conf: bool,
}

#[derive(Deserialize)]
struct Ddns {
    id: String,
    #[serde(default)]
    ip: Option<String>,
    #[serde(default)]
    update: Option<String>,
    #[serde(default)]
    running: bool,
}

pub(super) fn parse(raw: Raw) -> ServiceFacts {
    let ddns = raw
        .ddns
        .into_iter()
        .map(|d| {
            // `.update` is 0 until a first update; it counts from boot.
            let at = d
                .update
                .and_then(|u| u.trim().parse::<i64>().ok())
                .filter(|&u| u > 0);
            let updated_at = match (raw.now, raw.uptime, at) {
                (Some(now), Some(uptime), Some(at)) if at <= uptime => {
                    Some(Timestamp::from_unix_secs(now - uptime + at))
                }
                _ => None,
            };
            DdnsFacts {
                id: d.id,
                address: d.ip.and_then(|ip| ip.trim().parse::<IpAddr>().ok()),
                updated_at,
                running: d.running,
            }
        })
        .collect();
    let header = raw.blocklist.as_deref().and_then(parse_header);
    let (sha256, domains) = header.map_or((None, None), |(sha, n)| (Some(sha), Some(n)));
    ServiceFacts {
        ddns,
        doh_instances: raw.doh,
        blocklist: BlocklistFacts {
            sha256,
            domains,
            conf: raw.blocklist_conf,
        },
    }
}

#[cfg(test)]
mod tests {
    use uniwrt_core::Timestamp;

    use super::parse;

    #[test]
    fn updates_are_placed_on_the_clock_and_odd_files_skipped() {
        let raw = serde_json::from_str(
            r#"{"now": 1700000000, "uptime": 5000, "doh": 2, "ddns": [
                {"id": "home", "ip": "203.0.113.7", "update": "4000", "running": true},
                {"id": "six", "ip": "2001:db8::1\n", "update": "0", "running": false},
                {"id": "new", "ip": null, "update": null, "running": false},
                {"id": "odd", "ip": "nonsense", "update": "9999", "running": false}
            ]}"#,
        )
        .unwrap();
        let facts = parse(raw);
        assert_eq!(facts.doh_instances, Some(2));
        assert_eq!(
            facts.blocklist,
            uniwrt_model::facts::BlocklistFacts::default()
        );
        let home = &facts.ddns[0];
        assert_eq!(home.address, Some("203.0.113.7".parse().unwrap()));
        assert_eq!(
            home.updated_at,
            Some(Timestamp::from_unix_secs(1_699_999_000))
        );
        assert!(home.running);
        assert_eq!(facts.ddns[1].address, Some("2001:db8::1".parse().unwrap()));
        assert_eq!(facts.ddns[1].updated_at, None);
        assert_eq!(facts.ddns[2].address, None);
        // An update after the uptime is from before a reboot.
        assert_eq!(
            (facts.ddns[3].address, facts.ddns[3].updated_at),
            (None, None)
        );
    }

    #[test]
    fn the_blocklist_is_named_by_its_header() {
        let sha = "ab".repeat(32);
        let raw = serde_json::json!({
            "blocklist": format!("# uniwrt {sha} 1200"), "blocklist_conf": true,
        });
        let facts = parse(serde_json::from_value(raw).unwrap());
        assert_eq!(facts.blocklist.sha256, Some(sha));
        assert_eq!(facts.blocklist.domains, Some(1200));
        assert!(facts.blocklist.conf);
        // A file without the header (cut short, or someone else's) names nothing.
        let raw = serde_json::json!({"blocklist": "server=/a.com/", "blocklist_conf": false});
        let facts = parse(serde_json::from_value(raw).unwrap());
        assert_eq!(
            (facts.blocklist.sha256, facts.blocklist.conf),
            (None, false)
        );
    }
}
