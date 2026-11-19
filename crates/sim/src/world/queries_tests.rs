//! The gateway's query log as the controller reads it: every query paired with its answer,
//! blocked names only from the blocklist and never for a bypassed client, and the rest of the
//! world unchanged by it.

use std::collections::{BTreeSet, HashMap};

use uniwrt_core::Timestamp;
use uniwrt_model::dns_queries::{Outcome, Pairing, parse};

use super::QueryLog;
use crate::{SimConfig, world::World};

const START: Timestamp = Timestamp::from_unix_ms(1_793_245_539_000);
const BLOCKED: [&str; 2] = ["ads.tracker.example", "beacon.spy.example"];

fn log(world: &World) -> QueryLog {
    QueryLog {
        blocked: BLOCKED.map(str::to_owned).to_vec(),
        bypass: BTreeSet::from([world.clients[0].mac]),
        domain: "lan".to_owned(),
    }
}

#[test]
fn queries_pair_with_answers_and_leave_the_world_alone() {
    let config = SimConfig::default();
    let (mut quiet, mut logging) = (World::new(&config, START), World::new(&config, START));
    logging.set_query_log(Some(log(&logging)));
    let bypassed = logging.clients[0].ip;
    let first = logging.nodes[0].logd.records.back().map_or(0, |r| r.id);
    for _ in 0..600 {
        assert_eq!(quiet.step(), logging.step());
    }

    let mut pairing = Pairing::new("lan");
    let mut outcomes: HashMap<Outcome, usize> = HashMap::new();
    let mut pids = BTreeSet::new();
    for record in logging.nodes[0]
        .logd
        .records
        .iter()
        .filter(|r| r.id > first)
    {
        let Some((tag, message)) = record.msg.split_once(": ") else {
            continue;
        };
        let Some((program, pid)) = tag.split_once('[') else {
            continue;
        };
        let Some(line) = parse(Some(program), message) else {
            continue;
        };
        pids.insert(pid.trim_end_matches(']').to_owned());
        let at = Timestamp::from_unix_ms(record.time_ms);
        for query in pairing.line(&pid.to_owned(), at, line) {
            if query.outcome == Outcome::Blocked {
                assert!(BLOCKED.contains(&query.domain.as_str()), "{query:?}");
                assert_ne!(query.client, bypassed, "a bypassed client is never blocked");
            }
            *outcomes.entry(query.outcome).or_default() += 1;
        }
    }
    for outcome in [
        Outcome::Forwarded,
        Outcome::Cached,
        Outcome::Local,
        Outcome::Blocked,
    ] {
        assert!(
            outcomes.get(&outcome) > Some(&0),
            "{outcome:?} in {outcomes:?}"
        );
    }
    assert!(pids.contains("2092"), "{pids:?}");
}
