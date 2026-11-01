//! Where one node's log comes from, and what to skip so that each entry is stored once
//! (D-059): the agent follows the log until syslog arrives from the node, and again once its
//! syslog stops (TCP closed, or quiet for a while over UDP). While both run, around a switch,
//! an entry one source delivered is skipped when the other delivers it within [`OVERLAP`]; when
//! the agent starts following, its backfill is skipped through the newest entry stored.

use std::{collections::VecDeque, time::Duration};

use tokio::time::Instant;
use uniwrt_model::log::{LogEntry, LogVia};

use crate::Interest;

/// How long an entry one source delivered may come again from the other.
pub(super) const OVERLAP: Duration = Duration::from_secs(30);
/// The most recent entries kept to compare with.
const RECENT: usize = 256;

/// What two sources' copies of one entry share: logd's message as both send it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Key {
    program: Option<String>,
    pid: Option<u32>,
    message: String,
}

impl Key {
    pub(super) fn of(entry: &LogEntry) -> Self {
        Self {
            program: entry.program.clone(),
            pid: entry.pid,
            message: entry.message.clone(),
        }
    }

    fn matches(&self, entry: &LogEntry) -> bool {
        self.pid == entry.pid && self.program == entry.program && self.message == entry.message
    }
}

/// One node's log.
#[derive(Debug, Default)]
pub(super) struct NodeLog {
    /// Held while the agent follows the log.
    pub(super) agent: Option<Interest>,
    /// While syslog delivers the log: when it last did, and whether over TCP.
    pub(super) syslog: Option<(Instant, bool)>,
    /// The agent's next batch is skipped through the last entry like this.
    skip_through: Option<Key>,
    /// The newest entry stored.
    last: Option<Key>,
    recent: VecDeque<(Key, LogVia, Instant)>,
}

impl NodeLog {
    /// A node whose newest stored entry is `last`.
    pub(super) fn new(last: Option<Key>) -> Self {
        Self {
            last,
            ..Self::default()
        }
    }

    /// The agent follows the log again (or for the first time): its backfill repeats what was
    /// stored.
    pub(super) fn follow(&mut self, interest: Option<Interest>) {
        self.agent = interest;
        self.syslog = None;
        self.skip_through.clone_from(&self.last);
    }

    /// Syslog delivered an entry at `now`; true when the agent was following until now.
    pub(super) fn syslog_arrived(&mut self, now: Instant, tcp: bool) -> bool {
        let tcp = tcp || self.syslog.is_some_and(|(_, tcp)| tcp);
        self.syslog = Some((now, tcp));
        self.skip_through = None;
        self.agent.take().is_some()
    }

    /// Whether syslog went quiet over UDP for `quiet` by `now`.
    pub(super) fn syslog_quiet(&self, now: Instant, quiet: Duration) -> bool {
        self.syslog
            .is_some_and(|(last, tcp)| !tcp && now.duration_since(last) >= quiet)
    }

    /// Whether syslog came over TCP.
    pub(super) fn syslog_tcp(&self) -> bool {
        self.syslog.is_some_and(|(_, tcp)| tcp)
    }

    /// The entries of an agent batch to store.
    pub(super) fn agent_batch(&mut self, entries: &[LogEntry], now: Instant) -> Vec<LogEntry> {
        let mut entries = entries;
        if let Some(key) = self.skip_through.take()
            && let Some(repeated) = entries.iter().rposition(|entry| key.matches(entry))
        {
            entries = &entries[repeated + 1..];
        }
        entries
            .iter()
            .filter(|entry| self.keep(entry, LogVia::Agent, now))
            .cloned()
            .collect()
    }

    /// Whether to store an entry `via` delivered at `now`.
    pub(super) fn keep(&mut self, entry: &LogEntry, via: LogVia, now: Instant) -> bool {
        while self
            .recent
            .front()
            .is_some_and(|(_, _, at)| now.duration_since(*at) > OVERLAP)
        {
            self.recent.pop_front();
        }
        let copy = self
            .recent
            .iter()
            .position(|(key, from, _)| *from != via && key.matches(entry));
        if let Some(copy) = copy {
            // Each copy pairs with one entry: a message logged twice is kept twice.
            self.recent.remove(copy);
            return false;
        }
        let key = Key::of(entry);
        self.last = Some(key.clone());
        if self.recent.len() == RECENT {
            self.recent.pop_front();
        }
        self.recent.push_back((key, via, now));
        true
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::time::Instant;
    use uniwrt_core::Timestamp;
    use uniwrt_model::log::{LogEntry, LogVia, Severity};

    use super::{Key, NodeLog, OVERLAP};

    fn entry(message: &str, seq: u64) -> LogEntry {
        LogEntry {
            seq: Some(seq),
            time: Timestamp::from_unix_ms(i64::try_from(seq).unwrap()),
            facility: 3,
            severity: Severity::Info,
            program: Some("netifd".into()),
            pid: None,
            message: message.into(),
        }
    }

    fn texts(entries: &[LogEntry]) -> Vec<&str> {
        entries.iter().map(|e| e.message.as_str()).collect()
    }

    #[test]
    fn the_first_agent_batch_skips_through_the_newest_stored_entry() {
        let now = Instant::now();
        let batch = [entry("a", 1), entry("b", 2), entry("c", 3), entry("d", 4)];
        let mut log = NodeLog::new(Some(Key::of(&entry("b", 0))));
        log.follow(None);
        assert_eq!(texts(&log.agent_batch(&batch, now)), ["c", "d"]);
        // Only the first batch.
        assert_eq!(texts(&log.agent_batch(&[entry("b", 5)], now)), ["b"]);
        // When the newest stored entry isn't in the backfill, all of it is new.
        let mut log = NodeLog::new(Some(Key::of(&entry("z", 0))));
        log.follow(None);
        assert_eq!(log.agent_batch(&batch, now).len(), 4);
    }

    #[test]
    fn copies_from_the_other_source_are_skipped_around_a_switch() {
        let start = Instant::now();
        let mut log = NodeLog::new(None);
        log.follow(None);
        assert_eq!(
            texts(&log.agent_batch(&[entry("a", 1), entry("b", 2)], start)),
            ["a", "b"]
        );
        // Syslog arrives with b (already stored), then c; the agent's last poll brings c too.
        assert!(!log.syslog_arrived(start, false));
        assert!(!log.keep(&entry("b", 0), LogVia::Syslog, start));
        assert!(log.keep(&entry("c", 0), LogVia::Syslog, start));
        assert_eq!(log.agent_batch(&[entry("c", 3)], start), Vec::new());
        // A line logged twice is stored twice.
        assert!(log.keep(&entry("x", 0), LogVia::Syslog, start));
        assert!(log.keep(&entry("x", 0), LogVia::Syslog, start));
        assert_eq!(
            texts(&log.agent_batch(&[entry("x", 4), entry("x", 5)], start)),
            Vec::<&str>::new()
        );
        // Long after, the same text is a new entry.
        let later = start + OVERLAP + Duration::from_secs(1);
        assert!(log.keep(&entry("c", 0), LogVia::Agent, later));
    }

    #[test]
    fn syslog_takes_over_and_hands_back() {
        let start = Instant::now();
        let quiet = Duration::from_mins(10);
        let mut log = NodeLog::new(None);
        assert!(!log.syslog_quiet(start, quiet));
        log.keep(&entry("old", 0), LogVia::Agent, start);
        assert!(log.syslog_arrived(start, false) || log.agent.is_none());
        log.keep(&entry("s1", 0), LogVia::Syslog, start);
        assert!(!log.syslog_quiet(start + quiet / 2, quiet));
        assert!(log.syslog_quiet(start + quiet, quiet));
        // Back to the agent: its backfill is skipped through what syslog stored last.
        log.follow(None);
        assert_eq!(log.syslog, None);
        let later = start + quiet;
        let batch = [entry("old", 1), entry("s1", 2), entry("new", 3)];
        assert_eq!(texts(&log.agent_batch(&batch, later)), ["new"]);
        // Over TCP, syslog never goes quiet: the connection says when it ends.
        log.syslog_arrived(later, true);
        log.syslog_arrived(later, false);
        assert!(log.syslog_tcp());
        assert!(!log.syslog_quiet(later + quiet * 10, quiet));
    }
}
