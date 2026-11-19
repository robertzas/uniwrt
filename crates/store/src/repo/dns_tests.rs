//! DNS query statistics over a real database: stored, counted by the hour, read and pruned.

use std::time::Duration;

use uniwrt_core::{MacAddr, Timestamp};
use uniwrt_model::dns_queries::{DnsHour, DnsQuery, Outcome};

use crate::{
    DnsPrune, DnsQueryRepo as _,
    testing::{START, open},
};

fn query(at: Timestamp, domain: &str, mac: Option<MacAddr>, outcome: Outcome) -> DnsQuery {
    DnsQuery {
        at,
        client: "fd00::20".parse().unwrap(),
        mac,
        qtype: "AAAA".to_owned(),
        domain: domain.to_owned(),
        outcome,
    }
}

fn hour(hour: Timestamp, client: &str, queries: u64, blocked: u64) -> DnsHour {
    DnsHour {
        hour,
        client: client.to_owned(),
        queries,
        blocked,
    }
}

#[tokio::test]
async fn queries_are_kept_and_hours_add_up() {
    let (_dir, store, _clock) = open().await;
    let mac = Some(MacAddr::new([2, 0, 0, 0, 0x10, 1]));
    let later = START.saturating_add(Duration::from_secs(5));
    let queries = vec![
        query(START, "a.example", mac, Outcome::Blocked),
        query(later, "b.example", None, Outcome::Cached),
    ];
    // Counts arrive in parts: the same hour and client add up, wherever in the hour.
    let mid = START.saturating_add(Duration::from_mins(20));
    store
        .record_dns(queries.clone(), vec![hour(START, "c1", 3, 1)])
        .await
        .unwrap();
    store
        .record_dns(
            Vec::new(),
            vec![hour(mid, "c1", 2, 2), hour(mid, "c2", 1, 0)],
        )
        .await
        .unwrap();
    store.record_dns(Vec::new(), Vec::new()).await.unwrap();
    assert_eq!(
        store.dns_queries(START, 10).await.unwrap(),
        vec![queries[1].clone(), queries[0].clone()]
    );
    assert_eq!(store.dns_queries(later, 10).await.unwrap().len(), 1);
    assert_eq!(
        store.dns_queries(START, 1).await.unwrap(),
        vec![queries[1].clone()]
    );
    let start_hour = Timestamp::from_unix_ms(START.as_unix_ms().div_euclid(3_600_000) * 3_600_000);
    assert_eq!(
        store.dns_hours(mid).await.unwrap(),
        vec![hour(start_hour, "c1", 5, 3), hour(start_hour, "c2", 1, 0)]
    );
}

#[tokio::test]
async fn pruning_drops_old_queries_and_hours() {
    let (_dir, store, _clock) = open().await;
    let day = Duration::from_hours(24);
    let old = START.saturating_sub(day * 2);
    store
        .record_dns(
            vec![
                query(old, "old.example", None, Outcome::Forwarded),
                query(START, "new.example", None, Outcome::Forwarded),
            ],
            vec![
                hour(START.saturating_sub(day * 31), "c1", 1, 0),
                hour(old, "c1", 1, 0),
            ],
        )
        .await
        .unwrap();
    let prune = DnsPrune {
        queries_before: START.saturating_sub(day),
        hours_before: START.saturating_sub(day * 30),
    };
    assert_eq!(store.prune_dns(prune).await.unwrap(), 2);
    assert_eq!(store.prune_dns(prune).await.unwrap(), 0);
    let left = store.dns_queries(old, 10).await.unwrap();
    assert_eq!(
        left.iter().map(|q| q.domain.as_str()).collect::<Vec<_>>(),
        ["new.example"]
    );
    assert_eq!(
        store
            .dns_hours(old.saturating_sub(day * 30))
            .await
            .unwrap()
            .len(),
        1
    );
}
