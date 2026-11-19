//! DNS query statistics over a real database: stored, counted by the hour, read and pruned.

use std::time::Duration;

use uniwrt_core::{MacAddr, Timestamp};
use uniwrt_model::dns_queries::{DnsHour, DnsQuery, Outcome};

use crate::{
    DnsClient, DnsPrune, DnsQueryFilter, DnsQueryRepo as _, Store,
    testing::{START, open},
};

/// The queries at or after `at`, newest first.
async fn since(store: &Store, at: Timestamp, limit: usize) -> Vec<DnsQuery> {
    let filter = DnsQueryFilter {
        since: Some(at),
        ..DnsQueryFilter::default()
    };
    let page = store.dns_queries(filter, None, limit).await.unwrap();
    page.into_iter().map(|(_, query)| query).collect()
}

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
        since(&store, START, 10).await,
        vec![queries[1].clone(), queries[0].clone()]
    );
    assert_eq!(since(&store, later, 10).await.len(), 1);
    assert_eq!(since(&store, START, 1).await, vec![queries[1].clone()]);
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
    let left = since(&store, old, 10).await;
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

#[tokio::test]
async fn pages_filter_by_client_outcome_and_domain_text() {
    let (_dir, store, _clock) = open().await;
    let mac = MacAddr::new([2, 0, 0, 0, 0x10, 1]);
    let at = |s| START.saturating_add(Duration::from_secs(s));
    store
        .record_dns(
            vec![
                query(at(0), "ads.tracker.example", Some(mac), Outcome::Blocked),
                query(at(1), "www.example.org", Some(mac), Outcome::Cached),
                query(at(2), "a_b.example", None, Outcome::Forwarded),
                query(at(3), "axb.example", None, Outcome::Forwarded),
            ],
            Vec::new(),
        )
        .await
        .unwrap();
    let domains = |page: Vec<(i64, DnsQuery)>| {
        page.into_iter()
            .map(|(_, query)| query.domain)
            .collect::<Vec<_>>()
    };
    let read = async |filter: DnsQueryFilter, before, limit| {
        store.dns_queries(filter, before, limit).await.unwrap()
    };

    let mine = DnsQueryFilter {
        client: Some(DnsClient::Mac(mac)),
        ..DnsQueryFilter::default()
    };
    assert_eq!(
        domains(read(mine.clone(), None, 10).await),
        ["www.example.org", "ads.tracker.example"]
    );
    let by_address = DnsQueryFilter {
        client: Some(DnsClient::Address("fd00::20".parse().unwrap())),
        ..DnsQueryFilter::default()
    };
    assert_eq!(
        domains(read(by_address, None, 10).await),
        ["axb.example", "a_b.example"],
        "an address names the queries no MAC was known for"
    );
    let blocked = DnsQueryFilter {
        outcome: Some(Outcome::Blocked),
        ..mine
    };
    assert_eq!(
        domains(read(blocked, None, 10).await),
        ["ads.tracker.example"]
    );
    let text = |text: &str| DnsQueryFilter {
        domain: Some(text.to_owned()),
        ..DnsQueryFilter::default()
    };
    assert_eq!(domains(read(text("A_B"), None, 10).await), ["a_b.example"]);
    assert_eq!(
        read(text("%"), None, 10).await.len(),
        0,
        "wildcards are text"
    );
    assert_eq!(read(text(""), None, 10).await.len(), 4);

    // Pages follow each other through the last row's time and id.
    let first = read(DnsQueryFilter::default(), None, 3).await;
    let (id, last) = first.last().unwrap();
    let rest = read(DnsQueryFilter::default(), Some((last.at, *id)), 3).await;
    assert_eq!(domains(rest), ["ads.tracker.example"]);
}

#[tokio::test]
async fn totals_sum_hours_and_clients_and_top_domains_count_queries() {
    let (_dir, store, _clock) = open().await;
    let hour_ms = Duration::from_hours(1);
    let next = START.saturating_add(hour_ms);
    store
        .record_dns(
            vec![
                query(START, "ads.example", None, Outcome::Blocked),
                query(START, "ads.example", None, Outcome::Blocked),
                query(START, "spy.example", None, Outcome::Blocked),
                query(START, "www.example", None, Outcome::Cached),
                query(next, "www.example", None, Outcome::Forwarded),
                query(next, "mail.example", None, Outcome::Local),
            ],
            vec![
                hour(START, "c1", 4, 3),
                hour(START, "c2", 1, 0),
                hour(next, "c2", 6, 1),
            ],
        )
        .await
        .unwrap();
    let totals = store.dns_totals(START).await.unwrap();
    let start_hour = Timestamp::from_unix_ms(START.as_unix_ms().div_euclid(3_600_000) * 3_600_000);
    assert_eq!(
        totals
            .hours
            .iter()
            .map(|h| (h.hour, h.queries, h.blocked))
            .collect::<Vec<_>>(),
        [
            (start_hour, 5, 3),
            (start_hour.saturating_add(hour_ms), 6, 1)
        ]
    );
    assert_eq!(
        totals
            .clients
            .iter()
            .map(|c| (c.client.as_str(), c.queries, c.blocked))
            .collect::<Vec<_>>(),
        [("c2", 7, 1), ("c1", 4, 3)],
        "most queries first"
    );
    let top = |blocked| {
        let store = &store;
        async move {
            store
                .dns_top_domains(START, blocked, 10)
                .await
                .unwrap()
                .into_iter()
                .map(|d| (d.domain, d.queries))
                .collect::<Vec<_>>()
        }
    };
    assert_eq!(
        top(true).await,
        [("ads.example".to_owned(), 2), ("spy.example".to_owned(), 1)]
    );
    assert_eq!(
        top(false).await,
        [
            ("www.example".to_owned(), 2),
            ("mail.example".to_owned(), 1)
        ]
    );
    assert_eq!(store.dns_top_domains(next, true, 10).await.unwrap(), []);
}
