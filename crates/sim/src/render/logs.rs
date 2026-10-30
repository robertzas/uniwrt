//! `log` batches as the agent pushes them: logd's entries when polled, or `logread -f -t`
//! lines when streamed (`<ctime> [<secs>.<ms>] <facility>.<severity> <msg>`).

use serde_json::{Value, json};

use crate::world::LogRecord;

const FACILITIES: [&str; 12] = [
    "kern", "user", "mail", "daemon", "auth", "syslog", "lpr", "news", "uucp", "cron", "authpriv",
    "ftp",
];
const SEVERITIES: [&str; 8] = [
    "emerg", "alert", "crit", "err", "warn", "notice", "info", "debug",
];
const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// A polled batch: logd's entries and how many were lost before them.
pub(crate) fn log_entries(records: &[&LogRecord], dropped: u64) -> Value {
    let entries: Vec<_> = records
        .iter()
        .map(|record| {
            json!({
                "msg": record.msg,
                "id": record.id,
                "priority": record.priority,
                "source": record.source,
                "time": record.time_ms,
            })
        })
        .collect();
    json!({"entries": entries, "dropped": dropped})
}

/// A streamed batch: logread's lines.
pub(crate) fn log_lines(records: &[&LogRecord]) -> Value {
    let lines: Vec<_> = records.iter().map(|record| line(record)).collect();
    json!({"lines": lines, "dropped": 0})
}

fn line(record: &LogRecord) -> String {
    let facility = FACILITIES
        .get(usize::from(record.priority >> 3))
        .copied()
        .unwrap_or("daemon");
    let severity = SEVERITIES[usize::from(record.priority & 7)];
    let msg = if record.source == 0 {
        format!("kernel: {}", record.msg)
    } else {
        record.msg.clone()
    };
    let secs = record.time_ms.div_euclid(1000);
    format!(
        "{} [{secs}.{:03}] {facility}.{severity} {msg}",
        ctime(secs),
        record.time_ms.rem_euclid(1000)
    )
}

/// `Thu Oct 29 03:40:30 2026`, in UTC (the nodes' zone).
fn ctime(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let of_day = secs.rem_euclid(86_400);
    let (year, month, day) = civil(days);
    format!(
        "{} {} {day:2} {:02}:{:02}:{:02} {year}",
        DAYS[days.rem_euclid(7) as usize],
        MONTHS[(month - 1) as usize],
        of_day / 3600,
        of_day / 60 % 60,
        of_day % 60
    )
}

/// Year, month and day of a count of days since 1970-01-01 (Howard Hinnant's algorithm).
fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::{ctime, line};
    use crate::world::LogRecord;

    #[test]
    fn times_read_as_logread_writes_them() {
        assert_eq!(ctime(0), "Thu Jan  1 00:00:00 1970");
        assert_eq!(ctime(1_793_245_230), "Thu Oct 29 03:40:30 2026");
        assert_eq!(ctime(951_782_400), "Tue Feb 29 00:00:00 2000");
        let record = LogRecord {
            id: 1,
            priority: 29,
            source: 1,
            time_ms: 1_793_245_230_849,
            msg: "hostapd: phy1-ap0: AP-STA-CONNECTED 02:00:00:00:04:00".to_owned(),
        };
        assert_eq!(
            line(&record),
            "Thu Oct 29 03:40:30 2026 [1793245230.849] daemon.notice hostapd: phy1-ap0: AP-STA-CONNECTED 02:00:00:00:04:00"
        );
    }
}
