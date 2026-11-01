//! `log` batches (`node-agent/logs.uc`): logd's entries when polled, `{id, priority, source,
//! time, msg}` with the time in ms and `msg` as `program[pid]: text`, or `logread -f -t` lines
//! when streamed, `<ctime> [<s>.<ms>] <facility>.<severity> <msg>`, where the kernel's messages
//! read `kern.<severity> kernel: <text>`.

use serde::Deserialize;
use uniwrt_core::Timestamp;
use uniwrt_model::log::{LogEntry, Severity, split_tag};

/// logd's source of the kernel's messages (`SOURCE_KLOG`), whose text has no tag.
const KERNEL: u8 = 0;

#[derive(Deserialize)]
pub(super) struct Raw {
    #[serde(default)]
    entries: Vec<Entry>,
    #[serde(default)]
    lines: Vec<String>,
    #[serde(default)]
    dropped: u64,
}

#[derive(Deserialize)]
struct Entry {
    id: u64,
    priority: u8,
    source: u8,
    time: i64,
    msg: String,
}

/// The entries, and how many were lost: those the agent counted, and lines that do not parse.
pub(super) fn parse(raw: Raw) -> (Vec<LogEntry>, u64) {
    let mut entries: Vec<_> = raw.entries.into_iter().map(entry).collect();
    let mut dropped = raw.dropped;
    for text in &raw.lines {
        match line(text) {
            Some(parsed) => entries.push(parsed),
            None => dropped += 1,
        }
    }
    (entries, dropped)
}

fn entry(raw: Entry) -> LogEntry {
    let (program, pid, message) = if raw.source == KERNEL {
        (Some("kernel".to_owned()), None, raw.msg)
    } else {
        split_tag(&raw.msg)
    };
    LogEntry {
        seq: Some(raw.id),
        time: Timestamp::from_unix_ms(raw.time),
        facility: raw.priority >> 3,
        severity: Severity::from_code(raw.priority),
        program,
        pid,
        message,
    }
}

/// `Thu Oct 29 03:40:30 2026 [1793245230.849] daemon.info hostapd: phy3-ap0: …`.
fn line(text: &str) -> Option<LogEntry> {
    let (_, rest) = text.split_once(" [")?;
    let (stamp, rest) = rest.split_once("] ")?;
    let (secs, ms) = stamp.split_once('.')?;
    let ms = secs
        .parse::<i64>()
        .ok()?
        .checked_mul(1000)?
        .checked_add(ms.parse().ok()?)?;
    let (priority, msg) = rest.split_once(' ').unwrap_or((rest, ""));
    let (facility, severity) = priority.split_once('.')?;
    let (program, pid, message) = split_tag(msg);
    Some(LogEntry {
        seq: None,
        time: Timestamp::from_unix_ms(ms),
        facility: facility_code(facility)?,
        severity: Severity::parse(severity)?,
        program,
        pid,
        message,
    })
}

/// The codes of the facility names musl's `facilitynames` gives logread.
fn facility_code(name: &str) -> Option<u8> {
    Some(match name {
        "kern" => 0,
        "user" => 1,
        "mail" => 2,
        "daemon" => 3,
        "auth" | "security" => 4,
        "syslog" => 5,
        "lpr" => 6,
        "news" => 7,
        "uucp" => 8,
        "cron" => 9,
        "authpriv" => 10,
        "ftp" => 11,
        _ => {
            let n: u8 = name.strip_prefix("local")?.parse().ok()?;
            return (n < 8).then_some(16 + n);
        }
    })
}
