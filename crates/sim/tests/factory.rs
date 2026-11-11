//! A factory-default gateway (`SimConfig::factory`; features/adoption.md, Path E): root has no
//! password until `passwd` gives it one, and the LAN follows the gateway's `network` when its
//! ports bounce, as the apply's make-before-break renumbering needs (D-081).

#![allow(clippy::unwrap_used)]

use std::time::Duration;

use uniwrt_core::SystemClock;
use uniwrt_model::node::NodeRole;
use uniwrt_node::{
    ExecRequest, NodeLink as _, root_password,
    txn::{self, Footprint, TxnId},
};
use uniwrt_sim::{SimConfig, SimFleet, SimLink};

const BRIDGE: &str = "\nconfig interface 'lan'\n\toption device 'br-lan'\n\toption proto 'static'\n\
                      \tlist ipaddr '10.0.0.1/24'\n\tlist ipaddr '192.168.1.1/24'\n";
const SETTLED: &str = "\nconfig interface 'lan'\n\toption device 'br-lan'\n\toption proto 'static'\n\
                       \tlist ipaddr '10.0.0.1/24'\n";

async fn run<T: serde::de::DeserializeOwned>(link: &SimLink, command: &txn::Command<T>) -> T {
    let output = link.exec(command.request.clone()).await.unwrap();
    command.parse(output).unwrap()
}

async fn text(link: &SimLink, argv: &[&str]) -> String {
    let output = link
        .exec(ExecRequest::new(argv.iter().copied()))
        .await
        .unwrap();
    String::from_utf8(output.check(argv[0]).unwrap().stdout).unwrap()
}

/// Where the controller's login comes from, as the apply reads it.
async fn client(link: &SimLink) -> String {
    let env = text(link, &["env"]).await;
    let connection = env
        .lines()
        .find_map(|line| line.strip_prefix("SSH_CONNECTION="))
        .unwrap();
    connection.split(' ').next().unwrap().to_owned()
}

/// Applies `network` in transaction `id` and bounces the LAN's ports.
async fn renumber(link: &SimLink, id: &TxnId, network: &'static str, timeout: Duration) {
    run(link, &txn::stage(id, []).unwrap()).await;
    run(link, &txn::put(id, "network", network).unwrap()).await;
    run(link, &txn::check(id)).await;
    run(link, &txn::apply(id, timeout)).await;
    run(
        link,
        &txn::bounce(["lan1"], Duration::from_secs(3)).unwrap(),
    )
    .await;
}

fn factory() -> (SimFleet, SimLink) {
    let fleet = SimFleet::spawn(&SimConfig::factory(7), &SystemClock);
    let link = fleet.link(0).unwrap();
    (fleet, link)
}

#[tokio::test(start_paused = true)]
async fn a_factory_gateway_has_no_root_password_until_passwd_sets_one() {
    let (fleet, link) = factory();
    let nodes = fleet.nodes();
    assert_eq!(nodes.len(), 1);
    assert_eq!(
        (
            nodes[0].name.as_str(),
            nodes[0].role,
            nodes[0].host.as_str()
        ),
        ("OpenWrt", NodeRole::Gateway, "192.168.1.1")
    );
    assert_eq!(link.hosts(), ["192.168.1.1"]);
    assert!(link.root_password_is(""));

    let board: serde_json::Value =
        serde_json::from_str(&text(&link, &["ubus", "call", "system", "board"]).await).unwrap();
    assert_eq!(board["hostname"], "OpenWrt");
    assert_eq!(board["model"], nodes[0].model.as_str());

    // Its WAN asks for an address untagged, which the simulated ISP (VLAN 201) doesn't offer.
    let ping = link
        .exec(ExecRequest::new(["ping", "-c", "3", "203.0.113.1"]))
        .await
        .unwrap();
    assert!(ping.check("ping").is_err());

    let output = link.exec(root_password::set("abcd-efgh")).await.unwrap();
    output.check("passwd").unwrap();
    assert!(link.root_password_is("abcd-efgh") && !link.root_password_is(""));

    // A node set up by hand keeps the demo's password.
    let demo = SimFleet::spawn(
        &SimConfig {
            reboot_every: None,
            ..SimConfig::default()
        },
        &SystemClock,
    );
    assert!(
        demo.link(0)
            .unwrap()
            .root_password_is(uniwrt_sim::ROOT_PASSWORD)
    );
}

#[tokio::test(start_paused = true)]
async fn the_lan_follows_the_gateway_when_its_ports_bounce() {
    let (fleet, link) = factory();
    run(&link, &Footprint::get().install()).await;
    assert_eq!(client(&link).await, "192.168.1.50");

    // The bridge: the gateway answers at both addresses; the hosts move once the ports bounce.
    let bridge = TxnId::new("j1").unwrap();
    run(&link, &txn::stage(&bridge, []).unwrap()).await;
    run(&link, &txn::put(&bridge, "network", BRIDGE).unwrap()).await;
    run(&link, &txn::check(&bridge)).await;
    run(&link, &txn::apply(&bridge, Duration::from_secs(120))).await;
    assert_eq!(link.hosts(), ["192.168.1.1", "10.0.0.1"]);
    assert_eq!(client(&link).await, "192.168.1.50");
    run(
        &link,
        &txn::bounce(["lan1"], Duration::from_secs(3)).unwrap(),
    )
    .await;
    assert_eq!(fleet.nodes()[0].host, "10.0.0.1");
    assert_eq!(link.hosts(), ["10.0.0.1", "192.168.1.1"]);
    assert_eq!(client(&link).await, "10.0.0.50");
    run(&link, &txn::confirm(&bridge)).await;

    // Settle: the old address goes.
    let settle = TxnId::new("j2").unwrap();
    run(&link, &txn::stage(&settle, []).unwrap()).await;
    run(&link, &txn::put(&settle, "network", SETTLED).unwrap()).await;
    run(&link, &txn::check(&settle)).await;
    run(&link, &txn::apply(&settle, Duration::from_secs(120))).await;
    run(&link, &txn::confirm(&settle)).await;
    assert_eq!(link.hosts(), ["10.0.0.1"]);
    assert_eq!(client(&link).await, "10.0.0.50");
}

#[tokio::test(start_paused = true)]
async fn a_rollback_brings_the_lan_back() {
    let (fleet, link) = factory();
    run(&link, &Footprint::get().install()).await;
    let id = TxnId::new("j1").unwrap();
    renumber(&link, &id, BRIDGE, Duration::from_secs(60)).await;
    assert_eq!(client(&link).await, "10.0.0.50");

    // Nobody confirms: the timer runs out, the files come back and the ports bounce again.
    tokio::time::sleep(Duration::from_secs(90)).await;
    assert_eq!(client(&link).await, "192.168.1.50");
    assert_eq!(fleet.nodes()[0].host, "192.168.1.1");
    assert_eq!(link.hosts(), ["192.168.1.1"]);
}
