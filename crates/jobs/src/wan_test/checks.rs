//! What the connectivity test reads (features/wan.md, Connectivity test): the agent's facts for
//! the link, the address, the route and IPv6, and commands on the gateway for the rest (`ping`,
//! `nslookup`, `wget`, BusyBox's or uclient-fetch's). Pure readings here; `run.rs` sequences
//! them.

use std::{
    net::{IpAddr, Ipv4Addr},
    time::Duration,
};

use uniwrt_model::{
    facts::{InterfaceFacts, NodeFacts},
    site::gateway::{WanConfig, WanIpv6, WanProto},
};
use uniwrt_node::{ExecOutput, ExecRequest, ExitStatus};

/// The public resolver names are checked against besides the ISP's.
pub(crate) const PUBLIC_DNS: IpAddr = IpAddr::V4(Ipv4Addr::new(9, 9, 9, 9));
/// The name resolved.
pub(crate) const NAME: &str = "openwrt.org";
/// The HTTPS site fetched.
pub(crate) const HTTPS: &str = "https://openwrt.org/";
/// The IPv6 host pinged (Quad9).
pub(crate) const PING6: &str = "2620:fe::fe";

/// netifd's errors that end a wait at once: the ISP answered, and said no.
const TERMINAL: [&str; 2] = ["AUTH_FAILED", "NO_DEVICE"];

/// Whether the WAN's port has a carrier: the port itself, or the VLAN device on it.
pub(crate) fn link(facts: &NodeFacts, port: &str) -> bool {
    facts
        .devices
        .iter()
        .any(|d| (d.name == port || d.name.starts_with(&format!("{port}."))) && d.carrier)
}

/// The interface IPv4 arrives on, if it exists yet (DS-Lite's AFTR from DHCPv6 adds `wan` late).
fn ipv4(facts: &NodeFacts) -> Option<&InterfaceFacts> {
    facts.interfaces.iter().find(|i| i.name == "wan")
}

fn ipv6_iface(facts: &NodeFacts) -> Option<&InterfaceFacts> {
    facts.interfaces.iter().find(|i| i.name == "wan6")
}

/// What the address step found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Address {
    /// Up, with what to show, such as "Lease 203.0.113.10/24".
    Up(String),
    /// Not yet.
    Waiting,
    /// netifd gave up with these errors.
    Refused(Vec<String>),
}

pub(crate) fn address(facts: &NodeFacts, wan: &WanConfig) -> Address {
    let iface = match wan.proto {
        WanProto::None => ipv6_iface(facts),
        _ => ipv4(facts),
    };
    let Some(iface) = iface else {
        return Address::Waiting;
    };
    if iface.errors.iter().any(|e| TERMINAL.contains(&e.as_str())) {
        return Address::Refused(iface.errors.clone());
    }
    if !iface.up {
        return Address::Waiting;
    }
    let what = match wan.proto {
        WanProto::Dhcp(_) => "Lease",
        WanProto::Pppoe { .. } => "PPP session",
        WanProto::Static { .. } => "Address",
        WanProto::DsLite { .. } => "DS-Lite tunnel",
        WanProto::None => "IPv6 address",
    };
    match (&wan.proto, iface.ipv4.first(), iface.ipv6.first()) {
        (WanProto::None, _, Some(v6)) => Address::Up(format!("{what} {v6}")),
        (WanProto::DsLite { .. }, _, _) => Address::Up(what.to_owned()),
        (_, Some(v4), _) => Address::Up(format!("{what} {v4}")),
        _ => Address::Waiting,
    }
}

/// netifd's errors on the WAN's interface, for the hint.
pub(crate) fn errors(facts: &NodeFacts) -> Vec<String> {
    ipv4(facts)
        .into_iter()
        .chain(ipv6_iface(facts))
        .flat_map(|i| i.errors.iter().cloned())
        .collect()
}

/// The WAN's default route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Route {
    Via(IpAddr),
    /// A tunnel's, without a next hop (DS-Lite).
    Tunnel,
}

pub(crate) fn route(facts: &NodeFacts, wan: &WanConfig) -> Option<Route> {
    match wan.proto {
        WanProto::None => ipv6_iface(facts)
            .and_then(|i| i.ipv6_gateway)
            .map(|gw| Route::Via(IpAddr::V6(gw))),
        WanProto::DsLite { .. } => ipv4(facts).filter(|i| i.up).map(|_| Route::Tunnel),
        _ => ipv4(facts)
            .and_then(|i| i.ipv4_gateway)
            .map(|gw| Route::Via(IpAddr::V4(gw))),
    }
}

/// The ISP's resolver, if the WAN got one.
pub(crate) fn isp_dns(facts: &NodeFacts) -> Option<IpAddr> {
    ipv4(facts)
        .into_iter()
        .chain(ipv6_iface(facts))
        .flat_map(|i| i.dns_servers.iter().copied())
        .next()
}

/// IPv6's state once `wan6` is up: the delegated prefix (DHCPv6) or the tunnel's prefix (6rd).
pub(crate) fn ipv6(facts: &NodeFacts, mode: &WanIpv6) -> Option<String> {
    let iface = ipv6_iface(facts).filter(|i| i.up)?;
    match mode {
        WanIpv6::Off => None,
        WanIpv6::Dhcpv6 { .. } => iface
            .delegated_prefixes
            .first()
            .map(|p| format!("Prefix {p} delegated")),
        WanIpv6::SixRd { .. } => iface
            .delegated_prefixes
            .first()
            .map(|p| format!("6rd prefix {p}"))
            .or_else(|| iface.ipv6.first().map(|a| format!("6rd address {a}"))),
    }
}

fn request(argv: &[&str], timeout: Duration) -> ExecRequest {
    let mut request = ExecRequest::new(argv.iter().copied());
    request.timeout = timeout;
    request.max_output = 16 * 1024;
    request
}

pub(crate) fn ping(host: &str, v6: bool) -> ExecRequest {
    let mut argv = vec!["ping"];
    if v6 {
        argv.push("-6");
    }
    argv.extend(["-c", "3", "-W", "2", host]);
    request(&argv, Duration::from_secs(15))
}

pub(crate) fn nslookup(server: IpAddr) -> ExecRequest {
    request(
        &["nslookup", NAME, &server.to_string()],
        Duration::from_secs(10),
    )
}

pub(crate) fn https() -> ExecRequest {
    request(
        &["wget", "-q", "--spider", "-T", "10", HTTPS],
        Duration::from_secs(20),
    )
}

pub(crate) fn succeeded(output: &ExecOutput) -> bool {
    output.status == ExitStatus::Code(0)
}

/// The command isn't there (`sh` exits 127).
pub(crate) fn missing(output: &ExecOutput) -> bool {
    output.status == ExitStatus::Code(127)
}

/// The average round trip `ping` printed (`round-trip min/avg/max = 1.1/2.3/3.0 ms`).
pub(crate) fn rtt(output: &ExecOutput) -> Option<String> {
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.lines().find(|l| l.contains("min/avg/max"))?;
    let avg = line.split('=').nth(1)?.trim().split('/').nth(1)?;
    Some(format!("{avg} ms"))
}

#[cfg(test)]
mod tests {
    use uniwrt_model::{
        facts::{Counters, DeviceFacts, DeviceKind, InterfaceFacts, NodeFacts},
        site::gateway::{DhcpClient, WanConfig, WanIpv6, WanProto},
    };
    use uniwrt_node::{ExecOutput, ExitStatus};

    use super::{Address, Route, address, ipv6, link, route, rtt};

    fn iface(name: &str, up: bool) -> InterfaceFacts {
        InterfaceFacts {
            name: name.to_owned(),
            proto: "dhcp".to_owned(),
            up,
            device: None,
            uptime_s: None,
            ipv4: Vec::new(),
            ipv6: Vec::new(),
            delegated_prefixes: Vec::new(),
            ipv4_gateway: None,
            ipv6_gateway: None,
            dns_servers: Vec::new(),
            errors: Vec::new(),
        }
    }

    fn device(name: &str, carrier: bool) -> DeviceFacts {
        DeviceFacts {
            name: name.to_owned(),
            kind: DeviceKind::Ethernet,
            mac: None,
            mtu: 1500,
            up: true,
            carrier,
            speed_mbps: None,
            full_duplex: None,
            master: None,
            counters: Counters::default(),
        }
    }

    #[test]
    fn readings() {
        let mut facts = NodeFacts {
            devices: vec![device("eth1", false), device("eth1.201", true)],
            ..NodeFacts::default()
        };
        assert!(link(&facts, "eth1"));
        assert!(!link(&facts, "eth2"));
        let wan = WanConfig::default();
        assert_eq!(address(&facts, &wan), Address::Waiting);
        let mut up = iface("wan", true);
        up.ipv4 = vec!["203.0.113.10/24".parse().unwrap()];
        up.ipv4_gateway = "203.0.113.1".parse().ok();
        facts.interfaces.push(up);
        assert_eq!(
            address(&facts, &wan),
            Address::Up("Lease 203.0.113.10/24".to_owned())
        );
        assert_eq!(
            route(&facts, &wan),
            Some(Route::Via("203.0.113.1".parse().unwrap()))
        );
        let pppoe = WanConfig {
            proto: WanProto::Pppoe {
                username: "u".to_owned(),
                password: None,
                service: None,
            },
            ..WanConfig::default()
        };
        facts.interfaces[0].errors = vec!["AUTH_FAILED".to_owned()];
        assert_eq!(
            address(&facts, &pppoe),
            Address::Refused(vec!["AUTH_FAILED".to_owned()])
        );
        let mut wan6 = iface("wan6", true);
        assert_eq!(ipv6(&NodeFacts::default(), &wan.ipv6), None);
        wan6.delegated_prefixes = vec!["2001:db8:1::/56".parse().unwrap()];
        facts.interfaces.push(wan6);
        assert_eq!(
            ipv6(&facts, &WanIpv6::Dhcpv6 { reqprefix: None }).as_deref(),
            Some("Prefix 2001:db8:1::/56 delegated")
        );
        let dslite = WanConfig {
            proto: WanProto::DsLite { aftr: None },
            ..WanConfig::default()
        };
        assert_eq!(route(&facts, &dslite), Some(Route::Tunnel));
        assert!(matches!(
            WanConfig::default().proto,
            WanProto::Dhcp(DhcpClient { .. })
        ));
    }

    #[test]
    fn ping_prints_its_round_trip() {
        let out = ExecOutput {
            status: ExitStatus::Code(0),
            stdout: b"3 packets transmitted, 3 packets received, 0% packet loss\nround-trip min/avg/max = 1.104/2.318/3.020 ms\n".to_vec(),
            stderr: Vec::new(),
        };
        assert_eq!(rtt(&out).as_deref(), Some("2.318 ms"));
    }
}
