//! DNS query statistics (features/network.md, Query statistics; D-133): the lines of dnsmasq's
//! query log never become log entries. Intake hands them here through a queue of their own
//! ([`Feed`]: a full queue drops lines, never blocking intake), where each query is paired with
//! its answer (`uniwrt_model::dns_queries::Pairing`, per node and dnsmasq process), given the
//! MAC of the client holding its address, and stored with its client's hourly counts through
//! [`DnsQueryRepo`] every few seconds; queries are kept 24 h, hourly counts 30 days. Queries
//! from the gateway itself (loopback) are left out: its own lookups, and the unfiltered
//! instance asking the main one for local names.

use std::{collections::HashMap, net::IpAddr, sync::Arc, time::Duration};

use tokio::{
    sync::mpsc,
    task::JoinHandle,
    time::{Instant, MissedTickBehavior},
};
use tracing::warn;
use uniwrt_core::{MacAddr, NodeId, Timestamp};
use uniwrt_model::{
    dns_queries::{self, DnsHour, DnsQuery, Outcome, Pairing},
    log::LogEntry,
};
use uniwrt_node::NodeLink;
use uniwrt_store::{DnsPrune, DnsQueryRepo, RevisionRepo};

use crate::Engine;

#[derive(Debug, Clone, Copy)]
pub struct DnsStatsConfig {
    /// How often completed queries and counts are stored.
    pub flush_every: Duration,
    /// How long a query may wait for its answer line before it counts as forwarded.
    pub answer_within: Duration,
    pub keep_queries: Duration,
    pub keep_hours: Duration,
    pub prune_every: Duration,
    /// How often the clients' addresses and the site's local domain are read again.
    pub refresh_every: Duration,
}

impl Default for DnsStatsConfig {
    fn default() -> Self {
        Self {
            flush_every: Duration::from_secs(2),
            answer_within: Duration::from_secs(5),
            keep_queries: Duration::from_hours(24),
            keep_hours: Duration::from_hours(30 * 24),
            prune_every: Duration::from_mins(15),
            refresh_every: Duration::from_secs(30),
        }
    }
}

/// Lines waiting for the task; more are dropped.
const QUEUE: usize = 8_192;
/// How often drops are reported in the log at most.
const WARN_EVERY: Duration = Duration::from_mins(1);
/// A node's clock this far from the controller's isn't trusted for a query's time.
const SKEW: Duration = Duration::from_mins(5);
/// The most prunes in one round.
const PRUNES_PER_ROUND: usize = 20;
/// The site's local domain until a revision says otherwise.
const DEFAULT_DOMAIN: &str = "lan";

/// Whether `entry` is a line of dnsmasq's query log.
pub(super) fn is_query(entry: &LogEntry) -> bool {
    dns_queries::parse(entry.program.as_deref(), &entry.message).is_some()
}

struct Line {
    node: NodeId,
    received: Timestamp,
    entry: LogEntry,
}

/// Intake's end of the queue.
pub(super) struct Feed {
    tx: mpsc::Sender<Line>,
    dropped: u64,
    warned: Option<Instant>,
}

impl Feed {
    /// Hands on a query-log line `node` logged, which arrived at `received`.
    pub(super) fn send(&mut self, node: NodeId, received: Timestamp, entry: LogEntry) {
        let line = Line {
            node,
            received,
            entry,
        };
        if self.tx.try_send(line).is_err() {
            self.dropped += 1;
            let now = Instant::now();
            if self
                .warned
                .is_none_or(|at| now.duration_since(at) >= WARN_EVERY)
            {
                self.warned = Some(now);
                warn!(
                    total = self.dropped,
                    "DNS query log lines dropped: more than the statistics keep up with"
                );
            }
        }
    }
}

/// Starts the task; it stores what it holds and ends once the feed is dropped.
pub(super) fn spawn<L, S>(
    engine: &Engine<L>,
    repo: S,
    config: DnsStatsConfig,
) -> (Feed, JoinHandle<()>)
where
    L: NodeLink,
    S: DnsQueryRepo + RevisionRepo + Clone + 'static,
{
    let (tx, rx) = mpsc::channel(QUEUE);
    let stats = Stats {
        pairing: Pairing::new(DEFAULT_DOMAIN),
        macs: HashMap::new(),
        queries: Vec::new(),
        hours: HashMap::new(),
    };
    let task = tokio::spawn(run(engine.clone(), repo, config, stats, rx));
    let feed = Feed {
        tx,
        dropped: 0,
        warned: None,
    };
    (feed, task)
}

struct Stats {
    pairing: Pairing<(NodeId, Option<u32>)>,
    /// Who holds each address: online clients over offline ones.
    macs: HashMap<IpAddr, MacAddr>,
    queries: Vec<DnsQuery>,
    /// (hour's start in ms, client) → (queries, blocked).
    hours: HashMap<(i64, String), (u64, u64)>,
}

impl Stats {
    fn take(&mut self, line: &Line) {
        let entry = &line.entry;
        let Some(parsed) = dns_queries::parse(entry.program.as_deref(), &entry.message) else {
            return;
        };
        let skew = entry.time.as_unix_ms().abs_diff(line.received.as_unix_ms());
        let at = if u128::from(skew) <= SKEW.as_millis() {
            entry.time
        } else {
            line.received
        };
        let done = self.pairing.line(&(line.node, entry.pid), at, parsed);
        self.count(done);
    }

    fn count(&mut self, done: Vec<DnsQuery>) {
        for mut query in done {
            if query.client.is_loopback() {
                continue;
            }
            query.mac = self.macs.get(&query.client).copied();
            let hour = query.at.as_unix_ms().div_euclid(3_600_000) * 3_600_000;
            let counts = self.hours.entry((hour, query.client_key())).or_default();
            counts.0 += 1;
            counts.1 += u64::from(query.outcome == Outcome::Blocked);
            self.queries.push(query);
        }
    }

    fn refresh_clients<L: NodeLink>(&mut self, engine: &Engine<L>) {
        self.macs.clear();
        let mut clients = engine.clients();
        // Online clients last, so theirs win an address an offline one held before.
        clients.sort_by_key(|client| client.online);
        for client in clients {
            for ip in &client.ips {
                self.macs.insert(*ip, client.mac);
            }
        }
    }

    async fn store<S: DnsQueryRepo>(&mut self, repo: &S) {
        if self.queries.is_empty() {
            return;
        }
        let queries = std::mem::take(&mut self.queries);
        let hours = self
            .hours
            .drain()
            .map(|((hour, client), (queries, blocked))| DnsHour {
                hour: Timestamp::from_unix_ms(hour),
                client,
                queries,
                blocked,
            })
            .collect();
        if let Err(err) = repo.record_dns(queries, hours).await {
            warn!(%err, "cannot store DNS queries");
        }
    }
}

async fn run<L, S>(
    engine: Engine<L>,
    repo: S,
    config: DnsStatsConfig,
    mut stats: Stats,
    mut rx: mpsc::Receiver<Line>,
) where
    L: NodeLink,
    S: DnsQueryRepo + RevisionRepo,
{
    let clock = Arc::clone(&engine.context().clock);
    let mut flush = tokio::time::interval(config.flush_every);
    let mut refresh = tokio::time::interval(config.refresh_every);
    let mut prune = tokio::time::interval(config.prune_every);
    for timer in [&mut flush, &mut refresh, &mut prune] {
        timer.set_missed_tick_behavior(MissedTickBehavior::Delay);
    }
    loop {
        tokio::select! {
            line = rx.recv() => match line {
                Some(line) => stats.take(&line),
                None => break,
            },
            _ = flush.tick() => {
                let before = clock.now().saturating_sub(config.answer_within);
                let expired = stats.pairing.expire(before);
                stats.count(expired);
                stats.store(&repo).await;
            }
            _ = refresh.tick() => {
                stats.refresh_clients(&engine);
                match repo.head_revision().await {
                    Ok(Some(head)) => stats.pairing.set_local_domain(&head.config.site.domain),
                    Ok(None) => {}
                    Err(err) => warn!(%err, "cannot read the site's local domain"),
                }
                let watched: Vec<NodeId> = engine.all().into_iter().map(|(node, _)| node).collect();
                stats.pairing.retain(|(node, _)| watched.contains(node));
            }
            _ = prune.tick() => prune_old(&repo, clock.now(), config).await,
        }
    }
    let expired = stats.pairing.expire(Timestamp::from_unix_ms(i64::MAX));
    stats.count(expired);
    stats.store(&repo).await;
}

async fn prune_old<S: DnsQueryRepo>(repo: &S, now: Timestamp, config: DnsStatsConfig) {
    let prune = DnsPrune {
        queries_before: now.saturating_sub(config.keep_queries),
        hours_before: now.saturating_sub(config.keep_hours),
    };
    for _ in 0..PRUNES_PER_ROUND {
        match repo.prune_dns(prune).await {
            Ok(0) => break,
            Ok(_) => {}
            Err(err) => {
                warn!(%err, "cannot prune the DNS queries");
                break;
            }
        }
    }
}
