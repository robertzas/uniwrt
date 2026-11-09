//! Finding the gateway to set up at first run (features/adoption.md, Path E): the controller
//! sits behind a factory-default OpenWrt router, which is its default gateway (or, from a
//! container on a bridge network, still at OpenWrt's 192.168.1.1 through the host). Each
//! candidate address is probed: a dropbear banner, its host key, then a login without a
//! password, which only factory defaults accept, to read the model and the release. Nothing
//! changes on it; adopting it is the adopt job (an empty password).

use std::net::Ipv4Addr;

use futures_util::future::join_all;
use serde::Deserialize;
use uniwrt_core::NodeId;
use uniwrt_node::{Credential, ExecRequest, HostKey, Target};
use uniwrt_store::NodeRepo as _;
use zeroize::Zeroizing;

use crate::{Dial, Remote, adopt::check, runner::Shared};

/// Where factory-default OpenWrt answers on its LAN.
pub const OPENWRT_DEFAULT: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 1);

const SSH_PORT: u16 = 22;

/// A router found at one of the candidate addresses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundGateway {
    pub target: Target,
    /// For the user to confirm before adopting it.
    pub host_key: HostKey,
    /// Root has no password, so it runs factory defaults (or was left open): it can be adopted
    /// without one.
    pub blank: bool,
    /// From `ubus call system board`, when the login worked.
    pub model: Option<String>,
    pub hostname: Option<String>,
    /// `OpenWrt 24.10.8`, when the login worked.
    pub release: Option<String>,
    /// Why the controller can't manage it (an old release, not OpenWrt), when that is known.
    pub unsupported: Option<String>,
    /// The node already adopted at this address.
    pub adopted: Option<NodeId>,
}

/// The addresses to look at: the controller's default gateway, then OpenWrt's default.
pub fn candidates(default_gateway: Option<Ipv4Addr>) -> Vec<Ipv4Addr> {
    let mut hosts: Vec<_> = default_gateway.into_iter().collect();
    if !hosts.contains(&OPENWRT_DEFAULT) {
        hosts.push(OPENWRT_DEFAULT);
    }
    hosts
}

/// The IPv4 default gateway in a `/proc/net/route` listing: the first default route through
/// a gateway (flags `RTF_UP | RTF_GATEWAY`), lowest metric first.
pub fn default_gateway(proc_net_route: &str) -> Option<Ipv4Addr> {
    const RTF_UP: u32 = 0x1;
    const RTF_GATEWAY: u32 = 0x2;
    proc_net_route
        .lines()
        .skip(1)
        .filter_map(|line| {
            let fields: Vec<_> = line.split_whitespace().collect();
            let (destination, gateway, flags, metric) = (
                fields.get(1)?,
                fields.get(2)?,
                fields.get(3)?,
                fields.get(6)?,
            );
            let flags = u32::from_str_radix(flags, 16).ok()?;
            let wanted = RTF_UP | RTF_GATEWAY;
            if *destination != "00000000" || flags & wanted != wanted {
                return None;
            }
            // The kernel prints the address in host byte order: little-endian on every
            // architecture the controller ships for.
            let gateway = Ipv4Addr::from(u32::from_str_radix(gateway, 16).ok()?.swap_bytes());
            Some((metric.parse::<u32>().ok()?, gateway))
        })
        .min_by_key(|(metric, _)| *metric)
        .map(|(_, gateway)| gateway)
}

/// Probes every candidate at once; the routers found, in the candidates' order.
pub(crate) async fn find(shared: &Shared, hosts: &[Ipv4Addr]) -> Vec<FoundGateway> {
    let Some(dial) = &shared.dial else {
        return Vec::new();
    };
    let nodes = shared.store.nodes().await.unwrap_or_default();
    let probes = hosts.iter().map(|host| {
        let target = Target::new(host.to_string(), SSH_PORT);
        async move { probe(dial.as_ref(), target).await }
    });
    join_all(probes)
        .await
        .into_iter()
        .flatten()
        .map(|mut found| {
            found.adopted = nodes
                .iter()
                .find(|node| node.host == found.target.host && node.port == found.target.port)
                .map(|node| node.id);
            found
        })
        .collect()
}

/// One candidate: `None` unless dropbear answers there.
async fn probe(dial: &dyn Dial, target: Target) -> Option<FoundGateway> {
    let banner = dial.banner(&target).await.ok().flatten()?;
    if !banner.to_ascii_lowercase().contains("dropbear") {
        return None;
    }
    let host_key = dial.probe(&target).await.ok()?;
    let blank = Credential::Password(Zeroizing::new(String::new()));
    let mut found = FoundGateway {
        target,
        host_key,
        blank: false,
        model: None,
        hostname: None,
        release: None,
        unsupported: None,
        adopted: None,
    };
    if let Ok(remote) = dial.connect(&found.target, &found.host_key, &blank).await {
        found.blank = true;
        read(remote.as_ref(), &mut found).await;
        remote.close().await;
    }
    Some(found)
}

/// What `ubus call system board` says.
#[derive(Deserialize)]
struct Board {
    model: Option<String>,
    hostname: Option<String>,
}

/// Reads the release and the board over a login; what can't be read stays unknown.
async fn read(remote: &dyn Remote, found: &mut FoundGateway) {
    let text = |argv: &'static [&'static str]| async move {
        let output = remote
            .exec(ExecRequest::new(argv.iter().copied()))
            .await
            .ok()?;
        let output = output.check(argv[0]).ok()?;
        String::from_utf8(output.stdout).ok()
    };
    if let Some(release) = text(&["cat", "/etc/openwrt_release"]).await {
        match check::release(&release) {
            Ok(release) => found.release = Some(release),
            Err(why) => found.unsupported = Some(why),
        }
    }
    if let Some(board) = text(&["ubus", "call", "system", "board"]).await
        && let Ok(board) = serde_json::from_str::<Board>(&board)
    {
        found.model = board.model.filter(|model| !model.is_empty());
        found.hostname = board.hostname.filter(|name| !name.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROUTE: &str = "\
Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT
wlan0\t00000000\t0100000A\t0003\t0\t0\t600\t00000000\t0\t0\t0
eth0\t00000000\t0101A8C0\t0003\t0\t0\t100\t00000000\t0\t0\t0
eth0\t0001A8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\t0\t0\t0
";

    #[test]
    fn the_default_gateway_is_the_default_route_with_the_lowest_metric() {
        assert_eq!(default_gateway(ROUTE), Some(Ipv4Addr::new(192, 168, 1, 1)));
        let lines: Vec<_> = ROUTE.lines().collect();
        let wifi_only = lines[..2].join("\n");
        assert_eq!(
            default_gateway(&wifi_only),
            Some(Ipv4Addr::new(10, 0, 0, 1))
        );
        let none = [lines[0], lines[3]].join("\n");
        assert_eq!(default_gateway(&none), None);
        assert_eq!(default_gateway(""), None);
    }

    #[test]
    fn candidates_put_the_default_gateway_first_and_openwrt_s_default_once() {
        let docker = Ipv4Addr::new(172, 17, 0, 1);
        assert_eq!(candidates(Some(docker)), [docker, OPENWRT_DEFAULT]);
        assert_eq!(candidates(Some(OPENWRT_DEFAULT)), [OPENWRT_DEFAULT]);
        assert_eq!(candidates(None), [OPENWRT_DEFAULT]);
    }
}

#[cfg(test)]
mod find_tests {
    use super::*;
    use crate::testing::setup;

    #[tokio::test]
    async fn a_factory_router_shows_its_model_and_release_and_others_only_their_key() {
        let setup = setup(true).await;
        let hosts = [OPENWRT_DEFAULT];
        setup.node.password.lock().unwrap().clear();
        let found = setup.jobs.find_gateways(&hosts).await.unwrap();
        assert_eq!(found.len(), 1);
        let router = &found[0];
        assert_eq!(router.target, Target::new("192.168.1.1", 22));
        assert_eq!(router.host_key, setup.node.host_key);
        assert!(router.blank);
        assert_eq!(
            router.model.as_deref(),
            Some("QEMU Standard PC (Q35 + ICH9, 2009)")
        );
        assert_eq!(router.hostname.as_deref(), Some("OpenWrt"));
        assert_eq!(router.release.as_deref(), Some("OpenWrt 24.10.8"));
        assert_eq!((&router.unsupported, router.adopted), (&None, None));
        assert!(
            !setup
                .node
                .commands()
                .iter()
                .any(|c| c == "passwd" || c == "sh"),
            "nothing changes"
        );

        *setup.node.password.lock().unwrap() = "set".to_owned();
        *setup.node.release.lock().unwrap() =
            "DISTRIB_ID='OpenWrt'\nDISTRIB_RELEASE='23.05.5'\n".to_owned();
        let found = setup.jobs.find_gateways(&hosts).await.unwrap();
        assert!(!found[0].blank);
        assert_eq!((&found[0].model, &found[0].release), (&None, &None));

        setup.node.password.lock().unwrap().clear();
        let found = setup.jobs.find_gateways(&hosts).await.unwrap();
        assert!(
            found[0]
                .unsupported
                .as_deref()
                .is_some_and(|why| why.contains("24.10 or later")),
            "{found:?}"
        );
    }

    #[tokio::test]
    async fn an_adopted_router_says_so_and_no_dial_finds_nothing() {
        let setup = setup(true).await;
        let (job, _) = setup
            .adopt(setup.request(Some(crate::testing::PASSWORD)))
            .await;
        let node = setup.store.node(job.node.unwrap()).await.unwrap().unwrap();
        let host: Ipv4Addr = node.host.parse().unwrap();
        let found = setup.jobs.find_gateways(&[host]).await.unwrap();
        assert_eq!(found[0].adopted, Some(node.id));

        let offline = crate::testing::setup(false).await;
        assert!(matches!(
            offline.jobs.find_gateways(&[host]).await,
            Err(crate::Error::Unavailable)
        ));
    }
}
