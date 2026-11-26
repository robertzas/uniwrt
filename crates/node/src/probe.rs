//! The WAN quality monitor's probes (D-144): a round is `probe.uc` from the agent's bundle,
//! run on the gateway over exec ([`round`]), so a slow round holds up no agent request; it
//! answers one JSON object, read back as a [`ProbeRound`] ([`parse_round`]). When the
//! internet stops answering, a [`traceroute`] toward an internet target tells how far packets
//! still get ([`last_hop`]).

use std::{net::IpAddr, time::Duration};

use serde::Deserialize;
use serde_json::json;
use uniwrt_core::Timestamp;
use uniwrt_model::wan_monitor::{
    DnsVia, PINGS, Probe, ProbeResult, ProbeRound, WanMonitorSettings,
};

use crate::{Error, ExecOutput, ExecRequest, Result, bundle::DIR};

/// How long a round may take: the pings, then the slowest lookups and fetches one after
/// another.
const ROUND_TIMEOUT: Duration = Duration::from_mins(1);
const TRACEROUTE_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_OUTPUT: usize = 256 * 1024;
/// The furthest a traceroute goes.
const MAX_HOPS: u8 = 12;

/// The exec request of one round of `settings`' targets.
pub fn round(settings: &WanMonitorSettings) -> ExecRequest {
    let (mut icmp, mut dns, mut http) = (Vec::new(), Vec::new(), Vec::new());
    for target in &settings.targets {
        let key = &target.id;
        match &target.probe {
            Probe::Icmp { host } => icmp.push(json!({ "key": key, "host": host })),
            Probe::Dns { via, name } => {
                let server = match via {
                    DnsVia::Gateway => None,
                    DnsVia::Upstreams => Some("upstreams".to_owned()),
                    DnsVia::Server { address } => Some(address.to_string()),
                };
                dns.push(json!({ "key": key, "server": server, "name": name }));
            }
            Probe::Http { url } => http.push(json!({ "key": key, "url": url })),
        }
    }
    let spec = json!({ "count": PINGS, "icmp": icmp, "dns": dns, "http": http });
    ExecRequest {
        argv: vec![
            "ucode".to_owned(),
            format!("{DIR}/probe.uc"),
            spec.to_string(),
        ],
        stdin: None,
        timeout: ROUND_TIMEOUT,
        max_output: MAX_OUTPUT,
    }
}

#[derive(Deserialize)]
struct Answer {
    route: bool,
    first_hop: Option<IpAddr>,
    results: Vec<ProbeResult>,
}

/// The round a `round` request's output holds, as of `at`.
pub fn parse_round(at: Timestamp, output: ExecOutput) -> Result<ProbeRound> {
    let output = output.check("the WAN probe")?;
    let answer: Answer = serde_json::from_slice(&output.stdout)
        .map_err(|err| Error::Protocol(format!("the WAN probe answered no round: {err}")))?;
    Ok(ProbeRound {
        at,
        route: answer.route,
        first_hop: answer.first_hop,
        results: answer.results,
    })
}

/// A traceroute toward `host` (an address or a name the settings validated): one probe a
/// hop, a second each.
pub fn traceroute(host: &str) -> ExecRequest {
    ExecRequest {
        argv: [
            "traceroute",
            "-n",
            "-m",
            &MAX_HOPS.to_string(),
            "-q",
            "1",
            "-w",
            "1",
            "--",
            host,
        ]
        .map(str::to_owned)
        .to_vec(),
        stdin: None,
        timeout: TRACEROUTE_TIMEOUT,
        max_output: MAX_OUTPUT,
    }
}

/// The furthest hop that answered a traceroute (BusyBox's `-n` lines: ` 3  10.0.0.1  6.4 ms`).
pub fn last_hop(stdout: &str) -> Option<IpAddr> {
    stdout.lines().rev().find_map(|line| {
        let mut words = line.split_whitespace();
        words.next()?.parse::<u8>().ok()?;
        words.next()?.parse::<IpAddr>().ok()
    })
}

#[cfg(test)]
mod tests {
    use serde_json::Value;
    use uniwrt_core::Timestamp;
    use uniwrt_model::wan_monitor::WanMonitorSettings;

    use super::{last_hop, parse_round, round, traceroute};
    use crate::{ExecOutput, ExitStatus};

    #[test]
    fn a_round_passes_its_spec_as_one_argument() {
        let request = round(&WanMonitorSettings::default());
        assert_eq!(
            request.argv[..2],
            ["ucode", "/var/run/uniwrt/agent/probe.uc"]
        );
        let spec: Value = serde_json::from_str(&request.argv[2]).unwrap();
        assert_eq!(spec["count"], 10);
        assert_eq!(
            spec["icmp"][0],
            serde_json::json!({ "key": "first-hop", "host": null })
        );
        assert_eq!(spec["icmp"][1]["host"], "1.1.1.1");
        assert_eq!(spec["dns"][0]["server"], Value::Null);
        assert_eq!(spec["dns"][1]["server"], "upstreams");
        assert_eq!(spec["http"][0]["key"], "web");
    }

    #[test]
    fn a_round_reads_back_and_a_failure_is_an_error() {
        let stdout = br#"{ "route": true, "first_hop": "172.17.0.1", "results": [ { "key": "cloudflare", "sent": 3, "rtt_ms": [ 8.2, 7.218 ], "error": null }, { "key": "bad", "sent": 3, "rtt_ms": [ ], "error": "bad address 'x'" } ] }"#;
        let ok = |stdout: &[u8]| ExecOutput {
            status: ExitStatus::Code(0),
            stdout: stdout.to_vec(),
            stderr: Vec::new(),
        };
        let at = Timestamp::from_unix_ms(5);
        let round = parse_round(at, ok(stdout)).unwrap();
        assert!(round.route);
        assert_eq!(round.first_hop, Some("172.17.0.1".parse().unwrap()));
        assert_eq!(round.results[0].rtt_ms, [8.2, 7.218]);
        assert_eq!(round.results[1].error.as_deref(), Some("bad address 'x'"));
        assert!(parse_round(at, ok(b"usage")).is_err());
        let failed = ExecOutput {
            status: ExitStatus::Code(2),
            stdout: Vec::new(),
            stderr: b"usage: probe.uc <spec as JSON>".to_vec(),
        };
        assert!(parse_round(at, failed).is_err());
    }

    #[test]
    fn the_last_hop_is_the_furthest_that_answered() {
        let text = "traceroute to 1.1.1.1 (1.1.1.1), 12 hops max, 46 byte packets
 1  172.17.0.1  0.005 ms
 2  10.0.0.1  1.716 ms
 3  *
 4  *
";
        assert_eq!(last_hop(text), Some("10.0.0.1".parse().unwrap()));
        assert_eq!(last_hop(" 1  *\n"), None);
        assert_eq!(
            traceroute("1.1.1.1").argv.last().map(String::as_str),
            Some("1.1.1.1")
        );
    }
}
