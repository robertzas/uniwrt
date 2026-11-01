//! Parsing one syslog message (RFC 5424, else RFC 3164 as BSD syslog and OpenWrt's logd send
//! it). Lenient, as receivers must be: what does not parse becomes text, and only an empty
//! message is refused.
//!
//! logd's remote format (`logread -r`): `<PRI>Mmm dd hh:mm:ss [hostname ]tag: text`, the time
//! in the node's zone (which the message doesn't name), or `<PRI>Mmm dd hh:mm:ss [s.ms]
//! [hostname ]…` with `log_timestamp`, whose seconds since the epoch are kept.

use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uniwrt_core::Timestamp;
use uniwrt_model::log::{LogEntry, Severity, split_tag};

/// The longest message kept, in bytes; longer ones are cut.
pub const MAX_MESSAGE: usize = 8 * 1024;

/// RFC 3164's default priority for a message without one: user.notice.
const DEFAULT_PRIORITY: u8 = 13;
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// A message as received: its entry, and the host it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// The host the message names, if any (RFC 5424's HOSTNAME, or RFC 3164's).
    pub hostname: Option<String>,
    /// Whether the message's own time is in the entry (RFC 5424, or logd's `[s.ms]`). Without
    /// it, the entry's time is when it arrived.
    pub timed: bool,
    pub entry: LogEntry,
}

/// Parses one message that arrived at `received`; `None` when it has no text at all.
pub fn parse(bytes: &[u8], received: Timestamp) -> Option<Message> {
    let bytes = &bytes[..bytes.len().min(MAX_MESSAGE)];
    let text = String::from_utf8_lossy(bytes);
    let text = text.trim_end_matches(['\n', '\r', '\0']);
    if text.trim().is_empty() {
        return None;
    }
    let (priority, rest) = priority(text).unwrap_or((DEFAULT_PRIORITY, text));
    let mut message = match rest.strip_prefix("1 ") {
        Some(rest) => rfc5424(rest, received),
        None => None,
    }
    .unwrap_or_else(|| rfc3164(rest, received));
    message.entry.facility = priority >> 3;
    message.entry.severity = Severity::from_code(priority);
    Some(message)
}

/// `<PRI>` with PRI from 0 to 191, and what follows.
fn priority(text: &str) -> Option<(u8, &str)> {
    let rest = text.strip_prefix('<')?;
    let (digits, rest) = rest.split_once('>')?;
    if digits.is_empty() || digits.len() > 3 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let priority: u8 = digits.parse().ok()?;
    (priority <= 191).then_some((priority, rest))
}

/// `TIMESTAMP HOSTNAME APP-NAME PROCID MSGID STRUCTURED-DATA [MSG]`, after the version.
fn rfc5424(rest: &str, received: Timestamp) -> Option<Message> {
    let mut fields = rest.splitn(6, ' ');
    let stamp = fields.next()?;
    let hostname = nil(fields.next()?);
    let app = nil(fields.next()?);
    let procid = nil(fields.next()?);
    let _msgid = fields.next()?;
    let rest = skip_structured_data(fields.next().unwrap_or(""))?;
    let text = rest.strip_prefix(' ').unwrap_or(rest);
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let time = match stamp {
        "-" => None,
        stamp => Some(
            OffsetDateTime::parse(stamp, &Rfc3339)
                .ok()
                .and_then(|time| i64::try_from(time.unix_timestamp_nanos() / 1_000_000).ok())
                .map(Timestamp::from_unix_ms)?,
        ),
    };
    Some(Message {
        hostname: hostname.map(str::to_owned),
        timed: time.is_some(),
        entry: entry(
            time.unwrap_or(received),
            app.map(str::to_owned),
            procid.and_then(|pid| pid.parse().ok()),
            text.to_owned(),
        ),
    })
}

/// RFC 5424's nil value.
fn nil(field: &str) -> Option<&str> {
    (field != "-" && !field.is_empty()).then_some(field)
}

/// What follows STRUCTURED-DATA: `-`, or elements `[id param="value"…]` whose values escape
/// `"`, `\` and `]` with a backslash.
fn skip_structured_data(text: &str) -> Option<&str> {
    if let Some(rest) = text.strip_prefix('-') {
        return Some(rest);
    }
    let mut rest = text;
    while let Some(element) = rest.strip_prefix('[') {
        let mut quoted = false;
        let mut escaped = false;
        let end = element.char_indices().find_map(|(at, c)| {
            match c {
                _ if escaped => escaped = false,
                '\\' if quoted => escaped = true,
                '"' => quoted = !quoted,
                ']' if !quoted => return Some(at),
                _ => {}
            }
            None
        })?;
        rest = &element[end + 1..];
    }
    (rest.len() < text.len()).then_some(rest)
}

/// `[TIMESTAMP ][[s.ms] ][HOSTNAME ]TAG: MSG`; what doesn't fit is the text.
fn rfc3164(rest: &str, received: Timestamp) -> Message {
    let mut rest = bsd_stamp(rest).unwrap_or(rest);
    let mut time = None;
    if let Some((ms, after)) = logd_stamp(rest) {
        time = Some(Timestamp::from_unix_ms(ms));
        rest = after;
    }
    let mut hostname = None;
    // A first word that is not a tag is the host's name: a tag ends with `:` or holds `[`.
    if let Some((word, after)) = rest.split_once(' ')
        && !word.is_empty()
        && !word.ends_with(':')
        && !word.contains('[')
        && word
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-_:".contains(&b))
        && !after.is_empty()
    {
        hostname = Some(word.to_owned());
        rest = after;
    }
    let (program, pid, text) = split_tag(rest);
    Message {
        hostname,
        timed: time.is_some(),
        entry: entry(time.unwrap_or(received), program, pid, text),
    }
}

/// `Mmm dd hh:mm:ss ` (the day padded with a space), and what follows.
fn bsd_stamp(text: &str) -> Option<&str> {
    let stamp = text.get(..16)?;
    let b = stamp.as_bytes();
    let digits = |range: std::ops::Range<usize>| b[range].iter().all(u8::is_ascii_digit);
    let ok = MONTHS.iter().any(|month| month.as_bytes() == &b[..3])
        && b[3] == b' '
        && (b[4] == b' ' || b[4].is_ascii_digit())
        && b[5].is_ascii_digit()
        && b[6] == b' '
        && digits(7..9)
        && b[9] == b':'
        && digits(10..12)
        && b[12] == b':'
        && digits(13..15)
        && b[15] == b' ';
    // Every byte checked is ASCII, so 16 is a character boundary.
    ok.then(|| &text[16..])
}

/// logd's `[1793245230.849] `: milliseconds since the epoch, and what follows.
fn logd_stamp(text: &str) -> Option<(i64, &str)> {
    let (stamp, rest) = text.strip_prefix('[')?.split_once("] ")?;
    let (secs, ms) = stamp.split_once('.')?;
    if ms.len() != 3 || !secs.bytes().chain(ms.bytes()).all(|b| b.is_ascii_digit()) {
        return None;
    }
    let ms = secs
        .parse::<i64>()
        .ok()?
        .checked_mul(1000)?
        .checked_add(ms.parse().ok()?)?;
    Some((ms, rest))
}

fn entry(time: Timestamp, program: Option<String>, pid: Option<u32>, message: String) -> LogEntry {
    LogEntry {
        seq: None,
        time,
        // Set from the priority by `parse`.
        facility: 0,
        severity: Severity::Notice,
        program,
        pid,
        message,
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use uniwrt_core::Timestamp;
    use uniwrt_model::log::Severity;

    use super::{Message, parse};

    const AT: Timestamp = Timestamp::from_unix_ms(1_800_000_000_000);

    fn parsed(text: &str) -> Message {
        parse(text.as_bytes(), AT).unwrap()
    }

    #[test]
    fn logd_messages_parse_with_and_without_their_extras() {
        // As logd sends them: no host name, the node's local time (not kept).
        let m = parsed(
            "<30>Oct 29 03:40:30 hostapd: phy1-ap0: STA 02:00:00:00:04:00 IEEE 802.11: associated\n",
        );
        assert_eq!(m.hostname, None);
        assert!(!m.timed);
        assert_eq!(m.entry.time, AT);
        assert_eq!((m.entry.facility, m.entry.severity), (3, Severity::Info));
        assert_eq!(m.entry.program.as_deref(), Some("hostapd"));
        assert_eq!(m.entry.pid, None);
        assert!(m.entry.message.starts_with("phy1-ap0: STA"));
        // With `log_hostname`, `log_timestamp` and a pid; the day padded with a space.
        let m = parsed(
            "<29>Nov  1 09:05:01 [1793533501.123] ap-hall netifd[1203]: Interface 'lan' is now up",
        );
        assert_eq!(m.hostname.as_deref(), Some("ap-hall"));
        assert!(m.timed);
        assert_eq!(m.entry.time, Timestamp::from_unix_ms(1_793_533_501_123));
        assert_eq!(m.entry.program.as_deref(), Some("netifd"));
        assert_eq!(m.entry.pid, Some(1203));
        assert_eq!(m.entry.message, "Interface 'lan' is now up");
        // The kernel's, with logd's `kernel: ` prefix, and NUL-terminated (`log_trailer_null`).
        let m = parsed(
            "<4>Nov  1 09:05:01 kernel: [ 12.345] br-lan: port 1(lan1) entered blocking state\0",
        );
        assert_eq!((m.entry.facility, m.entry.severity), (0, Severity::Warning));
        assert_eq!(m.entry.program.as_deref(), Some("kernel"));
        assert_eq!(
            m.entry.message,
            "[ 12.345] br-lan: port 1(lan1) entered blocking state"
        );
    }

    #[test]
    fn rfc5424_messages_keep_their_time_host_app_and_pid() {
        let m = parsed(
            "<165>1 2026-11-01T09:05:01.250+01:00 sw-core.lan sshd 4321 ID47 \
             [exampleSDID@32473 iut=\"3\" eventSource=\"App\\]lication\"][x@1 a=\"b\"] \u{feff}Login ok",
        );
        assert_eq!(m.hostname.as_deref(), Some("sw-core.lan"));
        assert!(m.timed);
        assert_eq!(m.entry.time, Timestamp::from_unix_ms(1_793_520_301_250));
        assert_eq!((m.entry.facility, m.entry.severity), (20, Severity::Notice));
        assert_eq!(m.entry.program.as_deref(), Some("sshd"));
        assert_eq!(m.entry.pid, Some(4321));
        assert_eq!(m.entry.message, "Login ok");
        // Nil values everywhere, and no message.
        let m = parsed("<14>1 - - - - - -");
        assert_eq!((m.hostname, m.timed, m.entry.time), (None, false, AT));
        assert_eq!((m.entry.program, m.entry.message), (None, String::new()));
    }

    #[test]
    fn whatever_else_arrives_is_kept_as_text() {
        // No priority: user.notice, all text (a first word followed by more is a host's name).
        let m = parsed("hello");
        assert_eq!((m.entry.facility, m.entry.severity), (1, Severity::Notice));
        assert_eq!((m.hostname, m.entry.message.as_str()), (None, "hello"));
        // A priority out of range is text too.
        assert_eq!(parsed("<192>x").entry.message, "<192>x");
        // A broken RFC 5424 header falls back to RFC 3164.
        let m = parsed("<13>1 notatime host app - - [unterminated");
        assert_eq!(m.entry.severity, Severity::Notice);
        assert!(m.entry.message.contains("unterminated"));
        // Nothing to keep.
        for empty in ["", "\n", " \0"] {
            assert_eq!(parse(empty.as_bytes(), AT), None);
        }
    }

    #[test]
    fn long_messages_are_cut() {
        let long = format!("<13>{}", "é".repeat(10_000));
        let m = parsed(&long);
        assert!(m.entry.message.len() <= super::MAX_MESSAGE);
    }

    proptest! {
        #[test]
        fn any_bytes_parse_without_panicking(bytes in proptest::collection::vec(any::<u8>(), 0..300)) {
            let _ = parse(&bytes, AT);
        }

        #[test]
        fn any_text_after_a_priority_parses(text in "\\PC{0,200}") {
            let _ = parse(format!("<13>1 {text}").as_bytes(), AT);
            let _ = parse(format!("<13>Jan  1 00:00:00 {text}").as_bytes(), AT);
        }
    }
}
