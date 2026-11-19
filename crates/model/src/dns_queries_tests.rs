use uniwrt_core::Timestamp;

use super::{Answer, DnsQuery, Line, Outcome, Pairing, is_blocked, parse};

const DNSMASQ: Option<&str> = Some("dnsmasq");

fn at(s: i64) -> Timestamp {
    Timestamp::from_unix_ms(s * 1000)
}

/// Feeds dnsmasq's lines (as logged by 2.93 on OpenWrt 24.10.8 and 25.12.6) to one process.
fn feed(pairing: &mut Pairing<u32>, lines: &[&str]) -> Vec<DnsQuery> {
    lines
        .iter()
        .flat_map(|line| {
            let parsed = parse(DNSMASQ, line).unwrap_or_else(|| panic!("not a query line: {line}"));
            pairing.line(&1, at(10), parsed)
        })
        .collect()
}

fn outcomes(queries: &[DnsQuery]) -> Vec<(&str, &str, Outcome)> {
    queries
        .iter()
        .map(|q| (q.qtype.as_str(), q.domain.as_str(), q.outcome))
        .collect()
}

#[test]
fn the_extra_form_reads_queries_and_answers() {
    let line = "1 192.168.1.20/42593 query[A] Ads.Blocked.test from 192.168.1.20";
    assert_eq!(
        parse(DNSMASQ, line),
        Some(Line::Query {
            serial: Some(1),
            qtype: "A",
            name: "Ads.Blocked.test",
            from: "192.168.1.20".parse().unwrap(),
        })
    );
    let line = "9 fd00::20/37769 config 192.168.1.5 is NXDOMAIN";
    assert_eq!(
        parse(DNSMASQ, line),
        Some(Line::Answer {
            serial: Some(9),
            name: "192.168.1.5",
            answer: Answer::Nxdomain,
        })
    );
    for (line, answer) in [
        (
            "5 127.0.0.1/1 config error is REFUSED (EDE: network error)",
            Answer::Failed,
        ),
        (
            "7 127.0.0.1/1 /etc/hosts localhost is 127.0.0.1",
            Answer::Local,
        ),
        (
            "2 10.0.0.2/1 forwarded example.com to 1.1.1.1",
            Answer::Forwarded,
        ),
        (
            "3 10.0.0.2/1 cached example.com is 93.184.216.34",
            Answer::Cached,
        ),
        (
            "4 10.0.0.2/1 config printer.lan is NODATA-IPv6",
            Answer::Local,
        ),
        (
            "6 10.0.0.2/1 DHCP laptop.lan is 192.168.1.30",
            Answer::Local,
        ),
    ] {
        assert!(
            matches!(parse(DNSMASQ, line), Some(Line::Answer { answer: a, .. }) if a == answer),
            "{line}"
        );
    }
    // Upstream replies and anything else numbered are the query log's, never log entries.
    for line in [
        "2 10.0.0.2/1 reply example.com is 93.184.216.34",
        "2 10.0.0.2/1 validation result is INSECURE",
    ] {
        assert_eq!(parse(DNSMASQ, line), Some(Line::Other), "{line}");
    }
}

#[test]
fn the_plain_form_reads_too_and_other_lines_stay_log_entries() {
    assert!(matches!(
        parse(DNSMASQ, "query[AAAA] example.com from 192.168.1.20"),
        Some(Line::Query {
            serial: None,
            qtype: "AAAA",
            ..
        })
    ));
    assert!(matches!(
        parse(DNSMASQ, "config ads.blocked.test is NXDOMAIN"),
        Some(Line::Answer {
            serial: None,
            answer: Answer::Nxdomain,
            ..
        })
    ));
    assert_eq!(
        parse(DNSMASQ, "reply example.com is 1.2.3.4"),
        Some(Line::Other)
    );
    for line in [
        "started, version 2.93 cachesize 150",
        "using nameserver 192.0.2.1#53",
        "read /etc/hosts - 8 names",
        "using only locally-known addresses for lan",
        "DHCPACK(br-lan) 192.168.1.20 02:00:00:00:10:01 laptop",
        "",
    ] {
        assert_eq!(parse(DNSMASQ, line), None, "{line}");
    }
    // Only dnsmasq's.
    assert_eq!(
        parse(Some("odhcpd"), "query[A] example.com from 192.168.1.20"),
        None
    );
    assert_eq!(parse(None, "query[A] example.com from 192.168.1.20"), None);
}

#[test]
fn blocked_names_are_told_from_local_and_reverse_ones() {
    assert!(is_blocked("A", "ads.example.com", "lan"));
    assert!(is_blocked("HTTPS", "ads.example.com.", "lan"));
    assert!(!is_blocked("A", "nope.lan", "lan"));
    assert!(!is_blocked("A", "NOPE.LAN", "lan"));
    assert!(!is_blocked("A", "lan", "lan"));
    assert!(!is_blocked("A", "plan", "lan")); // a single label
    assert!(is_blocked("A", "my.plan", "lan"));
    assert!(!is_blocked("A", "singlelabel", "lan"));
    assert!(!is_blocked("PTR", "5.1.168.192.in-addr.arpa", "lan"));
    assert!(!is_blocked("A", "192.168.1.5", "lan"));
}

#[test]
fn queries_pair_with_their_answers() {
    let mut pairing = Pairing::new("lan");
    let done = feed(
        &mut pairing,
        &[
            "1 192.168.1.20/42593 query[A] ads.blocked.test from 192.168.1.20",
            "1 192.168.1.20/42593 config ads.blocked.test is NXDOMAIN",
            "3 192.168.1.20/56252 query[A] nope.lan from 192.168.1.20",
            "3 192.168.1.20/56252 config nope.lan is NXDOMAIN",
            "5 192.168.1.20/39499 query[A] example.com from 192.168.1.20",
            "5 192.168.1.20/39499 config error is REFUSED (EDE: network error)",
            "9 192.168.1.20/37769 query[PTR] 5.1.168.192.in-addr.arpa from 192.168.1.20",
            "9 192.168.1.20/37769 config 192.168.1.5 is NXDOMAIN",
            "10 192.168.1.20/1 query[A] example.org from 192.168.1.20",
            "10 192.168.1.20/1 forwarded example.org to 1.1.1.1",
            "10 192.168.1.20/1 reply example.org is 1.2.3.4",
            "11 192.168.1.20/1 query[A] example.org from 192.168.1.20",
            "11 192.168.1.20/1 cached example.org is 1.2.3.4",
        ],
    );
    assert_eq!(
        outcomes(&done),
        [
            ("A", "ads.blocked.test", Outcome::Blocked),
            ("A", "nope.lan", Outcome::Local),
            ("A", "example.com", Outcome::Failed),
            ("PTR", "5.1.168.192.in-addr.arpa", Outcome::Local),
            ("A", "example.org", Outcome::Forwarded),
            ("A", "example.org", Outcome::Cached),
        ]
    );
    assert_eq!(
        done[0].client,
        "192.168.1.20".parse::<std::net::IpAddr>().unwrap()
    );
    assert_eq!((done[0].at, done[0].mac), (at(10), None));
}

#[test]
fn an_unanswered_query_completes_as_forwarded() {
    let mut pairing = Pairing::new("lan");
    // A query joining one in flight logs no answer: the next query completes it…
    let done = feed(
        &mut pairing,
        &[
            "query[A] a.example from 10.0.0.2",
            "query[A] b.example from 10.0.0.2",
            "config b.example is NXDOMAIN",
        ],
    );
    assert_eq!(
        outcomes(&done),
        [
            ("A", "a.example", Outcome::Forwarded),
            ("A", "b.example", Outcome::Blocked)
        ]
    );
    // … or its expiry.
    feed(&mut pairing, &["query[A] c.example from 10.0.0.2"]);
    assert_eq!(pairing.expire(at(10)), Vec::<DnsQuery>::new());
    assert_eq!(
        outcomes(&pairing.expire(at(11))),
        [("A", "c.example", Outcome::Forwarded)]
    );
    // An answer naming another query is no one's.
    let done = feed(
        &mut pairing,
        &[
            "query[A] d.example from 10.0.0.2",
            "config other.example is NXDOMAIN",
        ],
    );
    assert_eq!(done, Vec::<DnsQuery>::new());
}

#[test]
fn queries_seen_again_are_skipped_but_a_restart_counts() {
    let mut pairing = Pairing::new("lan");
    let lines = [
        "41 10.0.0.2/1 query[A] a.example from 10.0.0.2",
        "41 10.0.0.2/1 cached a.example is 1.2.3.4",
        "42 10.0.0.2/1 query[A] b.example from 10.0.0.2",
        "42 10.0.0.2/1 cached b.example is 1.2.3.4",
    ];
    assert_eq!(feed(&mut pairing, &lines).len(), 2);
    // The agent's backfill brings them again.
    assert_eq!(feed(&mut pairing, &lines), Vec::<DnsQuery>::new());
    // Another process keeps its own serials.
    let parsed = parse(DNSMASQ, lines[0]).unwrap();
    assert_eq!(pairing.line(&2, at(10), parsed), Vec::<DnsQuery>::new());
    let parsed = parse(DNSMASQ, lines[1]).unwrap();
    assert_eq!(pairing.line(&2, at(10), parsed).len(), 1);
    // A dnsmasq started again under the same pid counts from 1.
    let mut pairing = Pairing::new("lan");
    let far = [
        "200041 10.0.0.2/1 query[A] a.example from 10.0.0.2",
        "200041 10.0.0.2/1 cached a.example is 1.2.3.4",
    ];
    assert_eq!(feed(&mut pairing, &far).len(), 1);
    assert_eq!(feed(&mut pairing, &lines).len(), 2);
}
