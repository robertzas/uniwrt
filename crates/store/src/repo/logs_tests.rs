//! Logs over a real database: recording, filters and full-text search, pages, context, the
//! histogram, sources, retention by age and size, and saved searches.

use std::time::Duration;

use uniwrt_core::{LogId, NodeId, Timestamp, UserId};
use uniwrt_model::log::{LogEntry, LogFilter, LogVia, NewLog, Severity};

use super::fts_terms;
use crate::{
    Error, LogRepo as _, LogsPrune,
    testing::{START, open},
};

const GW: NodeId = NodeId::new(1);
const AP: NodeId = NodeId::new(2);

fn at(secs: u64) -> Timestamp {
    START.saturating_add(Duration::from_secs(secs))
}

fn log(
    node: Option<NodeId>,
    secs: u64,
    severity: Severity,
    program: &str,
    message: &str,
) -> NewLog {
    NewLog {
        received: at(secs),
        node,
        host: node.is_none().then(|| "switch".to_owned()),
        via: if node.is_some() {
            LogVia::Agent
        } else {
            LogVia::Syslog
        },
        entry: LogEntry {
            seq: Some(secs),
            time: at(secs),
            facility: 3,
            severity,
            program: Some(program.to_owned()),
            pid: None,
            message: message.to_owned(),
        },
        pattern: message.contains("RADAR").then(|| "dfs-radar".to_owned()),
    }
}

fn sample() -> Vec<NewLog> {
    vec![
        log(
            Some(GW),
            1,
            Severity::Info,
            "dnsmasq-dhcp",
            "DHCPACK(br-lan) 192.168.1.20 02:00:00:00:04:00 phone",
        ),
        log(
            Some(AP),
            2,
            Severity::Info,
            "hostapd",
            "phy1-ap0: STA 02:00:00:00:04:00 IEEE 802.11: associated",
        ),
        log(
            Some(AP),
            3,
            Severity::Warning,
            "hostapd",
            "phy1-ap0: DFS-RADAR-DETECTED freq=5500",
        ),
        log(None, 4, Severity::Error, "sshd", "Failed password for root"),
        log(
            Some(GW),
            5,
            Severity::Notice,
            "netifd",
            "Interface 'wan' is now up",
        ),
    ]
}

fn messages(records: &[uniwrt_model::log::LogRecord]) -> Vec<u64> {
    records.iter().map(|r| r.entry.seq.unwrap()).collect()
}

#[tokio::test]
async fn entries_are_stored_and_found_by_every_filter() {
    let (_dir, store, _clock) = open().await;
    let stored = store.record_logs(sample()).await.unwrap();
    assert_eq!(stored.len(), 5);
    assert!(stored.windows(2).all(|w| w[0].id < w[1].id));
    assert_eq!(store.record_logs(Vec::new()).await.unwrap(), Vec::new());

    let find = |filter: LogFilter| {
        let store = store.clone();
        async move { messages(&store.logs(filter, None, 100).await.unwrap()) }
    };
    // Newest first, and read back as stored.
    assert_eq!(find(LogFilter::default()).await, [5, 4, 3, 2, 1]);
    assert_eq!(
        store.logs(LogFilter::default(), None, 1).await.unwrap()[0],
        stored[4]
    );
    let filter = |f: fn(&mut LogFilter)| {
        let mut filter = LogFilter::default();
        f(&mut filter);
        filter
    };
    assert_eq!(find(filter(|f| f.nodes = vec![AP])).await, [3, 2]);
    assert_eq!(find(filter(|f| f.nodes = vec![GW, AP])).await, [5, 3, 2, 1]);
    assert_eq!(find(filter(|f| f.hosts = vec!["switch".into()])).await, [4]);
    // Nodes and hosts are one set of sources.
    let sources = filter(|f| {
        f.nodes = vec![AP];
        f.hosts = vec!["switch".into()];
    });
    assert_eq!(find(sources.clone()).await, [4, 3, 2]);
    assert_eq!(
        find(filter(
            |f| f.programs = vec!["hostapd".into(), "netifd".into()]
        ))
        .await,
        [5, 3, 2]
    );
    assert_eq!(
        find(filter(|f| f.min = Some(Severity::Warning))).await,
        [4, 3]
    );
    assert_eq!(find(filter(|f| f.annotated = true)).await, [3]);
    assert_eq!(
        find(filter(|f| {
            f.from = Some(at(2));
            f.to = Some(at(4));
        }))
        .await,
        [3, 2]
    );
    // Every filter matches what LogFilter::admits says, but for text.
    for f in [
        filter(|f| f.nodes = vec![AP]),
        filter(|f| f.min = Some(Severity::Notice)),
        filter(|f| f.programs = vec!["sshd".into()]),
        sources,
    ] {
        let expected: Vec<_> = stored
            .iter()
            .rev()
            .filter(|r| f.admits(r))
            .map(|r| r.entry.seq.unwrap())
            .collect();
        assert_eq!(find(f).await, expected);
    }
    // Full text: words (anywhere, any case), a MAC as a phrase, prefixes, programs, exclusions.
    let text = |text: &str| filter_text(text);
    assert_eq!(find(text("02:00:00:00:04:00")).await, [2, 1]);
    assert_eq!(find(text("PHONE")).await, [1]);
    assert_eq!(find(text("assoc*")).await, [2]);
    assert_eq!(find(text("hostapd")).await, [3, 2]);
    assert_eq!(find(text("02:00:00:00:04:00 -dhcpack")).await, [2]);
    assert_eq!(find(text("-hostapd -netifd")).await, [4, 1]);
    assert_eq!(find(text("\"is now up\"")).await, [5]);
    assert_eq!(find(text("\"now is\"")).await, Vec::<u64>::new());
    // Nothing to search for: no text filter.
    assert_eq!(find(text("-- \" ")).await.len(), 5);
    // Text with filters.
    let mut both = text("02:00:00:00:04:00");
    both.nodes = vec![GW];
    assert_eq!(find(both).await, [1]);
}

fn filter_text(text: &str) -> LogFilter {
    LogFilter {
        text: Some(text.to_owned()),
        ..LogFilter::default()
    }
}

#[tokio::test]
async fn pages_context_histogram_and_sources() {
    let (_dir, store, _clock) = open().await;
    let stored = store.record_logs(sample()).await.unwrap();
    // Pages go back from a cursor; entries at the same time are ordered by id.
    let same_time = store
        .record_logs(vec![
            log(Some(GW), 3, Severity::Info, "a", "x"),
            log(Some(GW), 3, Severity::Info, "b", "y"),
        ])
        .await
        .unwrap();
    let first = store.logs(LogFilter::default(), None, 3).await.unwrap();
    assert_eq!(
        first.iter().map(|r| r.id).collect::<Vec<_>>(),
        [stored[4].id, stored[3].id, same_time[1].id]
    );
    let last = first.last().unwrap();
    let next = store
        .logs(LogFilter::default(), Some((last.entry.time, last.id)), 3)
        .await
        .unwrap();
    assert_eq!(
        next.iter().map(|r| r.id).collect::<Vec<_>>(),
        [same_time[0].id, stored[2].id, stored[1].id]
    );

    // Context: across all sources, oldest first.
    let context = store.log_context(stored[2].id, 2).await.unwrap();
    assert_eq!(
        context.iter().map(|r| r.id).collect::<Vec<_>>(),
        [
            stored[0].id,
            stored[1].id,
            stored[2].id,
            same_time[0].id,
            same_time[1].id
        ]
    );
    assert_eq!(
        store.log_context(LogId::new(999), 2).await.unwrap(),
        Vec::new()
    );

    // Two-second buckets from 0 s to 6 s, by severity.
    let histogram = store
        .log_histogram(LogFilter::default(), at(0), at(6), Duration::from_secs(2))
        .await
        .unwrap();
    assert_eq!(histogram.len(), 3);
    assert_eq!(histogram[1].start, at(2));
    let count =
        |bucket: usize, severity: Severity| histogram[bucket].counts[usize::from(severity.code())];
    assert_eq!(count(0, Severity::Info), 1);
    assert_eq!(
        (count(1, Severity::Info), count(1, Severity::Warning)),
        (3, 1)
    );
    assert_eq!(
        (count(2, Severity::Error), count(2, Severity::Notice)),
        (1, 1)
    );
    let warnings = store
        .log_histogram(
            LogFilter {
                min: Some(Severity::Warning),
                ..LogFilter::default()
            },
            at(0),
            at(5),
            Duration::from_secs(2),
        )
        .await
        .unwrap();
    assert_eq!(
        warnings
            .iter()
            .map(|b| b.counts.iter().sum::<u32>())
            .collect::<Vec<_>>(),
        [0, 1, 1]
    );

    let sources = store.log_sources(at(0)).await.unwrap();
    assert_eq!(sources.hosts, ["switch"]);
    assert_eq!(
        sources.programs,
        ["a", "b", "dnsmasq-dhcp", "hostapd", "netifd", "sshd"]
    );
    assert_eq!(store.log_sources(at(5)).await.unwrap().programs, ["netifd"]);

    // A node's newest entry is the last stored, whatever its time.
    assert_eq!(
        store.last_log(GW).await.unwrap(),
        Some(same_time[1].clone())
    );
    assert_eq!(store.last_log(NodeId::new(9)).await.unwrap(), None);
}

#[tokio::test]
async fn retention_deletes_the_oldest_by_age_then_size() {
    let (_dir, store, _clock) = open().await;
    let logs: Vec<_> = (0..100)
        .map(|i| log(Some(GW), i, Severity::Info, "p", &"x".repeat(100)))
        .collect();
    store.record_logs(logs).await.unwrap();
    let ids = |store: &crate::Store| {
        let store = store.clone();
        async move {
            let records = store.logs(LogFilter::default(), None, 1_000).await.unwrap();
            (records.len(), records.last().map(|r| r.entry.seq.unwrap()))
        }
    };
    let big = u64::MAX;
    // By age: what arrived before 30 s goes; its index rows and sources too.
    let prune = |before: u64, max_bytes: u64| LogsPrune {
        before: at(before),
        max_bytes,
    };
    assert_eq!(store.prune_logs(prune(30, big)).await.unwrap(), 30);
    assert_eq!(ids(&store).await, (70, Some(30)));
    assert_eq!(store.prune_logs(prune(30, big)).await.unwrap(), 0);
    // The full-text index lost the deleted entries' rows too.
    let indexed = store
        .read(|conn| {
            Ok(conn.query_row(
                "SELECT count(*) FROM logs_fts WHERE logs_fts MATCH '\"x\" *'",
                [],
                |row| row.get::<_, i64>(0),
            )?)
        })
        .await
        .unwrap();
    assert_eq!(indexed, 70);
    // By size: each entry is estimated at 2 × 101 bytes of text plus 96.
    assert_eq!(store.prune_logs(prune(0, 298 * 20)).await.unwrap(), 50);
    assert_eq!(ids(&store).await, (20, Some(80)));
    // Old sources go with the age limit.
    assert_eq!(store.log_sources(at(0)).await.unwrap().programs.len(), 1);
    store.prune_logs(prune(1_000, big)).await.unwrap();
    assert_eq!(ids(&store).await, (0, None));
    assert_eq!(
        store.log_sources(at(0)).await.unwrap().programs,
        Vec::<String>::new()
    );
    // Ids keep growing after everything was deleted.
    let again = store
        .record_logs(vec![log(Some(GW), 1, Severity::Info, "p", "y")])
        .await
        .unwrap();
    assert!(again[0].id.get() > 100);
}

#[tokio::test]
async fn searches_are_saved_by_unique_name() {
    let (_dir, store, _clock) = open().await;
    let filter = LogFilter {
        nodes: vec![AP],
        min: Some(Severity::Warning),
        text: Some("radar".into()),
        ..LogFilter::default()
    };
    let saved = store
        .save_search("Radar".into(), filter.clone(), None)
        .await
        .unwrap();
    assert_eq!((saved.name.as_str(), saved.created_at), ("Radar", START));
    assert!(matches!(
        store
            .save_search("Radar".into(), LogFilter::default(), None)
            .await,
        Err(Error::Conflict { .. })
    ));
    // A user that does not exist is refused.
    assert!(
        store
            .save_search("x".into(), LogFilter::default(), Some(UserId::new(7)))
            .await
            .is_err()
    );
    store
        .save_search("All".into(), LogFilter::default(), None)
        .await
        .unwrap();
    let all = store.saved_searches().await.unwrap();
    assert_eq!(
        all.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        ["All", "Radar"]
    );
    assert_eq!(all[1], saved);
    assert!(store.delete_search(saved.id).await.unwrap());
    assert!(!store.delete_search(saved.id).await.unwrap());
    assert_eq!(store.saved_searches().await.unwrap().len(), 1);
}

#[test]
fn text_becomes_quoted_fts_phrases() {
    let terms = |text| fts_terms(text);
    assert_eq!(terms("a b"), (vec!["\"a\"".into(), "\"b\"".into()], vec![]));
    assert_eq!(
        terms(" \"x y\" -z w* "),
        (
            vec!["\"x y\"".into(), "\"w\" *".into()],
            vec!["\"z\"".into()]
        )
    );
    // Quotes can't break out, and words without letters or digits are skipped.
    assert_eq!(terms("a\"b"), (vec!["\"a\"\"b\"".into()], vec![]));
    assert_eq!(
        terms("\"unterminated"),
        (vec!["\"unterminated\"".into()], vec![])
    );
    assert_eq!(terms("- * \"\" :::"), (vec![], vec![]));
    assert_eq!(terms(&"w ".repeat(40)).0.len(), 16);
}
