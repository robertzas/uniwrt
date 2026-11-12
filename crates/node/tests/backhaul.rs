//! The `mtu` op on the hwsim lab's wired backhaul (features/mesh.md, Wired backhaul; D-094):
//! `bh0`, a veth pair at batman-adv's MTU between node a (25.12, the gateway) and node b
//! (24.10), in a `uw_bh` zone of each node's firewall, as render gives a gateway's. Each end finds the other
//! at 1532 bytes; a size above the device's MTU isn't pinged; and once b's end drops to 1500,
//! the veth drops a's probes and neither end passes. Ignored by default: `just test-wifi` runs
//! it with the lab's `UNIWRT_LAB_GW`, `UNIWRT_LAB_AP` and `UNIWRT_LAB_PASSWORD`.

// Test code throughout: clippy.toml allows unwrap in tests, but not in their helpers.
#![allow(clippy::unwrap_used)]

mod openwrt;

use std::{env, net::Ipv6Addr, time::Duration};

use openwrt::{adopt_at, link};
use uniwrt_node::{
    AgentLaunch, NodeLink as _, SshLink,
    proto::ops::{Exec, Mtu, MtuProbe, code},
};

const LONG: Duration = Duration::from_secs(30);
const PORT: &str = "bh0";

async fn lab_link(var: &str) -> SshLink {
    let address = env::var(var).unwrap_or_else(|_| panic!("{var}=<host:port>"));
    let password = env::var("UNIWRT_LAB_PASSWORD").expect("UNIWRT_LAB_PASSWORD");
    let (_, node) = adopt_at(&address, &password).await;
    link(&node, AgentLaunch::Bundle).await.0
}

async fn probe(link: &SshLink, size: u16) -> MtuProbe {
    let mtu = Mtu {
        dev: PORT.to_owned(),
        size,
    };
    link.request(&mtu, LONG).await.unwrap()
}

/// The node's link-local addresses on the backhaul, from `/proc/net/if_inet6`.
async fn link_local(link: &SshLink) -> Vec<Ipv6Addr> {
    let table = link
        .request(&Exec::new(["cat", "/proc/net/if_inet6"]), LONG)
        .await
        .unwrap();
    table
        .stdout
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let hex = u128::from_str_radix(fields.first()?, 16).ok()?;
            (fields.get(5) == Some(&PORT)).then(|| Ipv6Addr::from(hex))
        })
        .filter(Ipv6Addr::is_unicast_link_local)
        .collect()
}

async fn set_mtu(link: &SshLink, mtu: &str) {
    let set = Exec::new(["ip", "link", "set", PORT, "mtu", mtu]);
    assert_eq!(link.request(&set, LONG).await.unwrap().code, Some(0));
}

#[tokio::test]
#[ignore = "needs the hwsim lab: just test-wifi"]
async fn each_end_of_the_backhaul_finds_the_other_at_batmans_mtu() {
    let a = lab_link("UNIWRT_LAB_GW").await;
    let b = lab_link("UNIWRT_LAB_AP").await;
    let (a_addr, b_addr) = (link_local(&a).await, link_local(&b).await);
    assert!(
        !a_addr.is_empty() && !b_addr.is_empty(),
        "{a_addr:?} {b_addr:?}"
    );
    for (from, peer) in [(&a, &b_addr), (&b, &a_addr)] {
        let probed = probe(from, Mtu::BATMAN).await;
        assert!(probed.passed(), "{probed:?}");
        assert_eq!(probed.mtu, 1532);
        assert_eq!(&probed.peers, peer, "only the other end answers");
    }
    // Above the device's MTU the kernel would fragment the probe: it isn't sent.
    let above = probe(&a, Mtu::BATMAN + 1).await;
    assert!(above.peers.is_empty() && !above.passed(), "{above:?}");
    let missing = Mtu {
        dev: "uwnone0".to_owned(),
        size: Mtu::BATMAN,
    };
    let err = a.request(&missing, LONG).await.unwrap_err();
    assert!(err.to_string().contains(code::NOT_FOUND), "{err}");

    // b's end at 1500: the veth drops a's 1532-byte frames, and b's device can't send them.
    set_mtu(&b, "1500").await;
    let from_a = probe(&a, Mtu::BATMAN).await;
    let from_b = probe(&b, Mtu::BATMAN).await;
    set_mtu(&b, "1532").await;
    assert!(!from_a.passed(), "{from_a:?}");
    assert_eq!((from_b.mtu, from_b.passed()), (1500, false), "{from_b:?}");
    // The lab is as it was.
    let again = probe(&a, Mtu::BATMAN).await;
    assert!(again.passed(), "{again:?}");
    a.shutdown().await;
    b.shutdown().await;
}
