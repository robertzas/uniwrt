use super::{DiagTool, DnsRecordType, IpFamily};

fn ping(target: &str) -> DiagTool {
    DiagTool::Ping {
        target: target.to_owned(),
        family: None,
        count: 4,
    }
}

#[test]
fn hosts_and_addresses_are_accepted() {
    for target in [
        "openwrt.org",
        "1.1.1.1",
        "2606:4700::1111",
        "ap-garden",
        "router.lan.",
    ] {
        assert_eq!(ping(target).validate(), Ok(()), "{target}");
    }
}

#[test]
fn what_could_be_an_option_or_a_shell_word_is_refused() {
    for target in [
        "",
        "-c1",
        "a b",
        "a;b",
        "$(id)",
        "a..b",
        "-x.org",
        &"a".repeat(254),
    ] {
        assert!(ping(target).validate().is_err(), "{target:?}");
    }
    let too_many = DiagTool::Ping {
        target: "a".to_owned(),
        family: None,
        count: 101,
    };
    assert!(too_many.validate().is_err());
}

#[test]
fn lookups_take_service_names_and_a_server() {
    let lookup = |name: &str, server: Option<&str>| DiagTool::Dns {
        name: name.to_owned(),
        record: Some(DnsRecordType::Srv),
        server: server.map(str::to_owned),
    };
    assert_eq!(
        lookup("_sip._tcp.example.com", Some("9.9.9.9")).validate(),
        Ok(())
    );
    assert_eq!(lookup("1.1.1.1", None).validate(), Ok(()));
    assert!(lookup("example.com", Some("-x")).validate().is_err());
    assert!(lookup("exa mple.com", None).validate().is_err());
}

#[test]
fn only_http_urls_are_fetched() {
    let http = |url: &str| {
        DiagTool::Http {
            url: url.to_owned(),
        }
        .validate()
    };
    assert_eq!(http("https://openwrt.org/"), Ok(()));
    assert_eq!(http("http://192.168.1.1:8080/cgi-bin?x=1"), Ok(()));
    for url in [
        "ftp://x",
        "https://",
        "https:///path",
        "https://a b",
        "file:///etc/shadow",
        "-O",
    ] {
        assert!(http(url).is_err(), "{url}");
    }
}

#[test]
fn tools_read_as_the_api_spells_them() {
    let tool: DiagTool =
        serde_json::from_str(r#"{"tool":"ping","target":"1.1.1.1","family":"ipv6"}"#).unwrap();
    assert_eq!(
        tool,
        DiagTool::Ping {
            target: "1.1.1.1".to_owned(),
            family: Some(IpFamily::Ipv6),
            count: 10
        }
    );
    let tool: DiagTool =
        serde_json::from_str(r#"{"tool":"dns","name":"openwrt.org","record":"AAAA"}"#).unwrap();
    assert_eq!(tool.name(), "dns");
    assert!(serde_json::from_str::<DiagTool>(r#"{"tool":"rm","target":"/"}"#).is_err());
    assert!(
        serde_json::from_str::<DiagTool>(r#"{"tool":"http","url":"https://a","x":1}"#).is_err()
    );
}

#[test]
fn a_throughput_test_takes_its_defaults_and_bounds() {
    use super::iperf3::{Iperf3Direction, Iperf3Peer};
    let tool: DiagTool = serde_json::from_str(r#"{"tool":"iperf3","peer":{"node":2}}"#).unwrap();
    assert_eq!(
        tool,
        DiagTool::Iperf3 {
            peer: Iperf3Peer::Node(uniwrt_core::NodeId::new(2)),
            direction: Iperf3Direction::Send,
            seconds: 10,
            streams: 1,
        }
    );
    assert_eq!(tool.validate(), Ok(()));
    assert_eq!(tool.target(), "node 2");
    let tool: DiagTool = serde_json::from_str(
        r#"{"tool":"iperf3","peer":"controller","direction":"both","seconds":60,"streams":8}"#,
    )
    .unwrap();
    assert_eq!(tool.validate(), Ok(()));
    assert_eq!(tool.target(), "the controller");
    for (seconds, streams) in [(0, 1), (61, 1), (10, 0), (10, 9)] {
        let tool = DiagTool::Iperf3 {
            peer: Iperf3Peer::Controller,
            direction: Iperf3Direction::Receive,
            seconds,
            streams,
        };
        assert!(tool.validate().is_err(), "{seconds} s, {streams} streams");
    }
}
