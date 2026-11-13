//! The bench (`SimConfig::bench`; features/adoption.md, Path C): a factory-default access point
//! on the gateway's adoption port, which only the gateway reaches until the port is lent and the
//! device's LAN asks for a lease. Read through the node crate's own requests and parsers, as the
//! jobs read them.

#![allow(clippy::unwrap_used)]

use std::{net::Ipv4Addr, time::Duration};

use serde_json::json;
use uniwrt_core::SystemClock;
use uniwrt_model::facts::NodeFacts;
use uniwrt_node::{
    AgentEvent, ExecRequest, NodeLink as _,
    adoption_port::{self as port, Lent},
    proto::ops::Collected,
    txn::{self, Footprint, TxnId},
};
use uniwrt_sim::{SimConfig, SimFleet, SimLink};

const NONE: [String; 0] = [];
const FACTORY: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 1);
const SEGMENT: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 250);
const DHCP_LAN: &str = "\nconfig device\n\toption name 'br-lan'\n\toption type 'bridge'\n\
                        \tlist ports 'lan1'\n\nconfig interface 'lan'\n\toption device 'br-lan'\n\
                        \toption proto 'dhcp'\n";

async fn text(link: &SimLink, request: ExecRequest) -> String {
    let what = request.argv[0].clone();
    let output = link.exec(request).await.unwrap();
    String::from_utf8(output.check(&what).unwrap().stdout).unwrap()
}

async fn lend(gateway: &SimLink, request: ExecRequest) -> Option<Lent> {
    Lent::parse(&text(gateway, request).await)
}

async fn segment_up(gateway: &SimLink) -> bool {
    port::segment_up(&text(gateway, port::segment_status()).await, SEGMENT)
}

async fn run<T: serde::de::DeserializeOwned>(link: &SimLink, command: &txn::Command<T>) -> T {
    let output = link.exec(command.request.clone()).await.unwrap();
    command.parse(output).unwrap()
}

async fn leases(gateway: &SimLink) -> Vec<(String, String)> {
    let args = serde_json::value::to_raw_value(&json!({"sets": ["leases"]})).unwrap();
    let answer = gateway
        .call("collect", &args, Duration::from_secs(5))
        .await
        .unwrap();
    let collected: Collected = serde_json::from_str(answer.get()).unwrap();
    let mut facts = NodeFacts::default();
    let _ = uniwrt_node::facts::apply(&collected, &mut facts);
    facts
        .leases
        .iter()
        .filter_map(|lease| Some((lease.mac?.to_string(), lease.ip.to_string())))
        .collect()
}

#[tokio::test(start_paused = true)]
async fn the_bench_device_answers_on_the_segment_then_leases_while_the_port_is_lent() {
    let config = SimConfig {
        reboot_every: None,
        bench: true,
        ..SimConfig::default()
    };
    let fleet = SimFleet::spawn(&config, &SystemClock);
    assert_eq!(fleet.nodes().len(), 4, "the house's nodes only");
    assert_eq!(fleet.brownfield().len(), 4);
    assert!(fleet.brownfield()[0].files["network"].contains("option name 'br-adopt'"));
    let gateway = fleet.link(0).unwrap();
    let device = fleet.bench().unwrap();
    assert!(segment_up(&gateway).await);
    assert_eq!(device.segment_hosts(), NONE, "not plugged in yet");
    assert_eq!(device.hosts(), NONE);
    let topics = serde_json::value::to_raw_value(&json!({"topics": ["netifd"]})).unwrap();
    gateway
        .call("subscribe", &topics, Duration::from_secs(5))
        .await
        .unwrap();
    let mut events = gateway.events();

    // Plugged in: the gateway sees the port's link, and the device answers on the segment.
    tokio::time::sleep(Duration::from_secs(22)).await;
    let mut linked = false;
    while let Ok(event) = events.try_recv() {
        if let AgentEvent::Event { topic, data, .. } = event {
            linked |= topic == "netifd.device.link_up" && data.get().contains("\"lan4\"");
        }
    }
    assert!(linked, "the gateway reports the port's link");
    assert_eq!(device.segment_hosts(), [FACTORY.to_string()]);
    assert!(device.root_password_is(""));
    let mac = port::identity_mac(&text(&device, port::identity()).await).unwrap();
    let arp = text(&gateway, port::arp_table()).await;
    assert_eq!(port::neighbor(&arp, FACTORY, port::BRIDGE), Some(mac));
    let board = text(&device, port::board()).await;
    assert_eq!(port::Board::parse(&board).unwrap().hostname, "OpenWrt");

    // Lent while the device's LAN is static: off the segment, and no lease.
    let lan = || port::lend("lan4", "lan", Duration::from_secs(60));
    assert_eq!(lend(&gateway, lan()).await, Some(Lent::Lent));
    assert_eq!(lend(&gateway, lan()).await, Some(Lent::Busy));
    assert!(!segment_up(&gateway).await);
    assert_eq!(device.segment_hosts(), NONE);
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(device.hosts(), NONE);
    assert_eq!(
        lend(&gateway, port::end_lend("lan4")).await,
        Some(Lent::Ended)
    );
    assert!(segment_up(&gateway).await);

    // Its LAN asks for a lease: it leaves 192.168.1.1, and takes a lease once the port is lent.
    run(&device, &Footprint::get().install()).await;
    let id = TxnId::new("staging").unwrap();
    run(&device, &txn::stage(&id, []).unwrap()).await;
    run(&device, &txn::put(&id, "network", DHCP_LAN).unwrap()).await;
    run(&device, &txn::check(&id)).await;
    run(&device, &txn::apply(&id, Duration::from_mins(5))).await;
    assert_eq!(device.segment_hosts(), NONE);
    assert_eq!(lend(&gateway, lan()).await, Some(Lent::Lent));
    tokio::time::sleep(Duration::from_secs(2)).await;
    let [host] = &device.hosts()[..] else {
        panic!("one lease: {:?}", device.hosts());
    };
    assert!(host.starts_with("10.0.0."), "{host}");
    assert!(
        leases(&gateway)
            .await
            .contains(&(mac.to_string(), host.clone()))
    );
    assert_eq!(lend(&gateway, port::keep("lan4")).await, Some(Lent::Kept));
    tokio::time::sleep(Duration::from_secs(90)).await;
    assert_eq!(
        device.hosts(),
        std::slice::from_ref(host),
        "a kept lend lasts"
    );

    // The lend ends: off the LAN; the rollback brings 192.168.1.1 back on the segment.
    assert_eq!(
        lend(&gateway, port::end_lend("lan4")).await,
        Some(Lent::Ended)
    );
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(device.hosts(), NONE);
    run(&device, &txn::rollback(&id)).await;
    assert_eq!(device.segment_hosts(), [FACTORY.to_string()]);
}
