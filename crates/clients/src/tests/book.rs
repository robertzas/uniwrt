//! The book over builds: clients appearing, roaming, leaving and coming back; nodes going
//! quiet; a restart; aliases and groups; samples and retries; forgetting.

use uniwrt_core::{MacAddr, NodeId, Timestamp};
use uniwrt_model::{
    client::{ClientMedium, ClientRecord, ClientSession, SessionEnd},
    status::NodeState,
    wifi::Band,
};

use super::{AP, World, phone, private, secs};
use crate::{Book, BookConfig, ClientKind, Factor};

fn session(node: i64, start: Timestamp, end: Option<(Timestamp, SessionEnd)>) -> ClientSession {
    ClientSession {
        mac: phone(),
        node: Some(NodeId::new(node)),
        port: Some(AP.to_owned()),
        medium: ClientMedium::Wifi,
        band: Some(Band::G5),
        ssid: Some("home".to_owned()),
        start,
        end: end.map(|(at, _)| at),
        ended_by: end.map(|(_, by)| by),
    }
}

#[test]
fn a_client_joins_roams_leaves_and_comes_back() {
    let mut world = World::new();
    world.lease(phone(), "Annas-iPhone", None);
    world.associate(1, phone(), secs(-30));
    let changes = world.build(secs(10));
    assert_eq!(changes.sessions, [session(1, secs(-30), None)]);
    assert_eq!(changes.records.len(), 2, "first seen, then named");
    assert_eq!(changes.records[1].hostname.as_deref(), Some("Annas-iPhone"));
    let client = &changes.clients[0];
    assert!(client.online);
    assert_eq!(client.vendor.as_deref(), Some("Apple"));
    assert_eq!(client.kind, ClientKind::Phone);
    assert_eq!(
        client.ips,
        ["192.168.1.1".parse::<std::net::IpAddr>().unwrap()]
    );
    let connection = client.connection.as_ref().unwrap();
    assert_eq!(
        (
            connection.node,
            connection.port.as_deref(),
            connection.since
        ),
        (Some(NodeId::new(1)), Some(AP), secs(-30))
    );
    assert_eq!(
        (connection.channel, connection.noise_dbm),
        (Some(36), Some(-95))
    );
    assert_eq!(connection.upstream_name.as_deref(), Some("node1"));
    assert_eq!(client.experience.as_ref().unwrap().score, 100);

    // It roams to node 2.
    world.associate(2, phone(), secs(15));
    let changes = world.build(secs(20));
    assert_eq!(
        changes.sessions,
        [
            session(1, secs(-30), Some((secs(15), SessionEnd::Roamed))),
            session(2, secs(15), None)
        ]
    );
    let roaming = changes.clients[0].experience.as_ref().unwrap();
    let part = roaming
        .parts
        .iter()
        .find(|part| part.factor == Factor::Roaming);
    assert!((part.unwrap().value - 1.0).abs() < f32::EPSILON);

    // It leaves: the session ends when it was last seen.
    world.leave(phone());
    let changes = world.build(secs(30));
    assert_eq!(
        changes.sessions,
        [session(2, secs(15), Some((secs(20), SessionEnd::Left)))]
    );
    let client = &changes.clients[0];
    assert!(!client.online);
    assert_eq!(client.experience, None);
    assert_eq!(client.last_seen, secs(20));
    assert_eq!(
        client.connection.as_ref().unwrap().node,
        Some(NodeId::new(2))
    );
    assert_eq!(changes.records[0].last_seen, secs(20));

    // Back at node 2 with a new association, much later: a new session.
    world.associate(2, phone(), secs(100));
    let changes = world.build(secs(110));
    assert_eq!(changes.sessions, [session(2, secs(100), None)]);
}

#[test]
fn a_node_that_goes_quiet_ends_its_sessions_until_the_association_shows_again() {
    let mut world = World::new();
    world.associate(2, phone(), secs(100));
    world.build(secs(110));
    world.states[1] = NodeState::Offline;
    let changes = world.build(secs(120));
    assert_eq!(
        changes.sessions,
        [session(2, secs(100), Some((secs(110), SessionEnd::Left)))]
    );
    assert!(!changes.clients[0].online);
    // The same association, still on: the session goes on.
    world.states[1] = NodeState::Online;
    let changes = world.build(secs(200));
    assert_eq!(changes.sessions, [session(2, secs(100), None)]);
    assert!(changes.clients[0].online);
}

#[test]
fn showing_up_elsewhere_soon_after_leaving_is_a_roam() {
    let mut world = World::new();
    world.associate(1, phone(), secs(0));
    world.build(secs(10));
    world.leave(phone());
    world.build(secs(20));
    world.associate(2, phone(), secs(22));
    let changes = world.build(secs(25));
    assert_eq!(
        changes.sessions,
        [
            session(1, secs(0), Some((secs(10), SessionEnd::Roamed))),
            session(2, secs(22), None)
        ]
    );
    // Too late to be a roam.
    world.leave(phone());
    world.build(secs(30));
    world.associate(1, phone(), secs(60));
    let changes = world.build(secs(61));
    assert_eq!(changes.sessions, [session(1, secs(60), None)]);
}

#[test]
fn a_restored_session_goes_on_or_ends_when_the_client_was_last_seen() {
    let record = ClientRecord {
        mac: phone(),
        alias: Some("Anna's phone".to_owned()),
        hostname: Some("Annas-iPhone".to_owned()),
        first_seen: secs(-1000),
        last_seen: secs(50),
    };
    let mut book = Book::new(BookConfig::default());
    let changes = book.restore(vec![record.clone()], vec![session(1, secs(-30), None)]);
    let client = &changes.clients[0];
    assert!(!client.online);
    assert_eq!(client.alias.as_deref(), Some("Anna's phone"));
    assert_eq!(client.kind, ClientKind::Phone);
    assert_eq!(client.connection.as_ref().unwrap().since, secs(-30));

    let mut world = World::new();
    world.book = book;
    world.associate(1, phone(), secs(-30));
    let changes = world.build(secs(100));
    assert!(changes.sessions.is_empty(), "{:?}", changes.sessions);
    let detail = world.book.detail(phone()).unwrap();
    assert_eq!(detail.session, Some(session(1, secs(-30), None)));
    assert_eq!(detail.client.first_seen, secs(-1000));

    let mut world = World::new();
    world
        .book
        .restore(vec![record], vec![session(1, secs(-30), None)]);
    let changes = world.build(secs(100));
    assert_eq!(
        changes.sessions,
        [session(1, secs(-30), Some((secs(50), SessionEnd::Left)))]
    );
}

#[test]
fn randomized_addresses_of_one_device_share_its_alias() {
    let mut world = World::new();
    let (old, new) = (private(1), private(2));
    world.lease(old, "Annas-Pixel", Some("015a0000000001"));
    world.associate(1, old, secs(0));
    world.build(secs(10));
    let (record, changes) = world
        .book
        .rename(old, Some("Anna's Pixel".to_owned()))
        .unwrap();
    assert_eq!(record.alias.as_deref(), Some("Anna's Pixel"));
    assert_eq!(
        changes.clients[0].group.as_deref(),
        Some("name:annas-pixel")
    );
    assert!(changes.clients[0].randomized);
    assert_eq!(changes.clients[0].vendor, None);

    world.lease(new, "Annas-Pixel", None);
    world.associate(1, new, secs(20));
    let changes = world.build(secs(30));
    let view = changes
        .clients
        .iter()
        .find(|client| client.mac == new)
        .unwrap();
    assert_eq!(view.alias.as_deref(), Some("Anna's Pixel"));
    assert_eq!(world.book.rename(MacAddr::BROADCAST, None), None);

    // A generic hostname and a client ID that is just the address tie nothing.
    let mut world = World::new();
    world.lease(private(3), "iPhone", Some("015a0000000003"));
    world.associate(1, private(3), secs(0));
    let changes = world.build(secs(10));
    assert_eq!(changes.clients[0].group, None);
}

#[test]
fn samples_come_every_few_seconds_with_the_retry_rate_in_the_score() {
    let mut world = World::new();
    world.associate(1, phone(), secs(0));
    world.retries(phone(), 100, 0);
    assert_eq!(world.build(secs(10)).samples.len(), 1);
    assert_eq!(world.build(secs(12)).samples, []);
    world.retries(phone(), 200, 30);
    let changes = world.build(secs(15));
    assert_eq!(changes.samples.len(), 1);
    let (mac, sample) = changes.samples[0];
    assert_eq!(
        (mac, sample.at, sample.signal_dbm, sample.snr_db),
        (phone(), secs(15), Some(-55), Some(40))
    );
    let experience = changes.clients[0].experience.as_ref().unwrap();
    let retries = experience
        .parts
        .iter()
        .find(|part| part.factor == Factor::Retries)
        .unwrap();
    assert!((retries.value - 30.0).abs() < 0.01);
    assert_eq!(retries.score, 29);
    assert_eq!(world.book.detail(phone()).unwrap().samples.len(), 2);
}

#[test]
fn clients_long_gone_are_forgotten_unless_named() {
    let mut world = World::new();
    world.associate(1, phone(), secs(0));
    world.associate(1, private(9), secs(0));
    world.build(secs(10));
    world.book.rename(private(9), Some("Kept".to_owned()));
    world.leave(phone());
    world.leave(private(9));
    world.build(secs(20));
    let month = 30 * 24 * 3600;
    assert_eq!(world.book.prune(secs(month)).removed, []);
    assert_eq!(world.book.prune(secs(month + 11)).removed, [phone()]);
    assert_eq!(world.book.clients().len(), 1);
    let unsaved = world.book.unsaved();
    assert!(unsaved.records.is_empty(), "saved as they went offline");
    assert_eq!(
        unsaved.sessions.len(),
        1,
        "the last session of the one kept"
    );
}
