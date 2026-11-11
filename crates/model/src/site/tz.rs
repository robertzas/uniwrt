//! Time zones: the IANA names a site may use and the POSIX TZ string OpenWrt's
//! `system.timezone` takes for each (`data/tz.tsv`, built from tzdata by `just tz`).

use uniwrt_core::Timestamp;

use super::wireless::Weekday;

const TABLE: &str = include_str!("../../data/tz.tsv");

/// The POSIX TZ string of an IANA zone, such as `CET-1CEST,M3.5.0,M10.5.0/3` for
/// `Europe/Berlin`; `None` for a name tzdata doesn't have.
pub fn posix(zone: &str) -> Option<&'static str> {
    TABLE
        .lines()
        .filter(|line| !line.starts_with('#'))
        .find_map(|line| line.split_once('\t').filter(|(name, _)| *name == zone))
        .map(|(_, tz)| tz)
}

/// The standard time's offset from UTC of a POSIX TZ string, in seconds east (`CET-1CEST,…`
/// is 3600, `EST5EDT,…` is -18000); `None` when it doesn't parse. [`local`] knows daylight
/// saving time too.
pub fn standard_offset_s(posix: &str) -> Option<i32> {
    standard(posix).map(|(offset, _)| offset)
}

/// A moment as a zone's clocks show it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Local {
    pub day: Weekday,
    /// Minutes after midnight (0–1439).
    pub minute: u16,
    /// The zone's offset from UTC then, in seconds east, daylight saving time included.
    pub offset_s: i32,
}

/// `at` in the zone of a POSIX TZ string, by its daylight saving rule (`M<month>.<week>.<day>`
/// with an optional time, the only form tzdata's strings use); `None` when it doesn't parse.
pub fn local(posix: &str, at: Timestamp) -> Option<Local> {
    let utc_s = at.as_unix_ms().div_euclid(1_000);
    let offset_s = offset_at(posix, utc_s)?;
    let local_s = utc_s + i64::from(offset_s);
    let days = local_s.div_euclid(DAY_S);
    Some(Local {
        // 1970-01-01 was a Thursday.
        day: Weekday::from_monday(u8::try_from((days + 3).rem_euclid(7)).ok()?),
        minute: u16::try_from(local_s.rem_euclid(DAY_S) / 60).ok()?,
        offset_s,
    })
}

const DAY_S: i64 = 86_400;

/// The zone's offset from UTC at `utc_s`, in seconds east.
fn offset_at(posix: &str, utc_s: i64) -> Option<i32> {
    let (std, rest) = standard(posix)?;
    if rest.is_empty() {
        return Some(std);
    }
    // The daylight name, its offset (an hour ahead unless given) and the two changes.
    let rest = after_name(rest)?;
    let (dst, rest) = if rest.starts_with(',') {
        (std + 3_600, rest)
    } else {
        let (west, rest) = hms(rest)?;
        (-west, rest)
    };
    let (start, end) = rest.strip_prefix(',')?.split_once(',')?;
    let (start, end) = (Change::parse(start)?, Change::parse(end)?);
    // Each change is given on the clock it ends: the start in standard time, the end in
    // daylight time. South of the equator the year starts in daylight time.
    let year = year_of(utc_s + i64::from(std));
    let starts = start.at(year) - i64::from(std);
    let ends = end.at(year) - i64::from(dst);
    let daylight = if starts < ends {
        (starts..ends).contains(&utc_s)
    } else {
        !(ends..starts).contains(&utc_s)
    };
    Some(if daylight { dst } else { std })
}

/// The standard offset in seconds east, and what follows it.
fn standard(posix: &str) -> Option<(i32, &str)> {
    let (west, rest) = hms(after_name(posix)?)?;
    (west.abs() <= 24 * 3_600).then_some((-west, rest))
}

/// What follows a zone's abbreviation: letters, or anything in `<…>`.
fn after_name(text: &str) -> Option<&str> {
    if let Some(quoted) = text.strip_prefix('<') {
        return Some(&quoted[quoted.find('>')? + 1..]);
    }
    let rest = text.trim_start_matches(|c: char| c.is_ascii_alphabetic());
    (rest.len() < text.len()).then_some(rest)
}

/// A signed `h[:m[:s]]` in seconds, and what follows it.
fn hms(text: &str) -> Option<(i32, &str)> {
    let (sign, rest) = match text.as_bytes().first()? {
        b'-' => (-1, &text[1..]),
        b'+' => (1, &text[1..]),
        _ => (1, text),
    };
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == ':'))
        .unwrap_or(rest.len());
    let mut parts = rest[..end].split(':').map(str::parse::<i32>);
    let hours = parts.next()?.ok()?;
    let minutes = parts.next().transpose().ok()?.unwrap_or(0);
    let seconds = parts.next().transpose().ok()?.unwrap_or(0);
    (hours <= 167 && minutes < 60 && seconds < 60).then_some((
        sign * (hours * 3_600 + minutes * 60 + seconds),
        &rest[end..],
    ))
}

/// When the clocks change: `M<month>.<week>.<day>[/<time>]`, the `week`th `day` (0 is Sunday)
/// of the month, week 5 being its last, at `time` (02:00 unless given; tzdata uses -1 to 50 h).
struct Change {
    month: i64,
    week: i64,
    day: i64,
    time_s: i64,
}

impl Change {
    fn parse(text: &str) -> Option<Self> {
        let (date, time) = text.split_once('/').unwrap_or((text, "2"));
        let mut parts = date.strip_prefix('M')?.split('.').map(str::parse::<i64>);
        let month = parts.next()?.ok()?;
        let week = parts.next()?.ok()?;
        let day = parts.next()?.ok()?;
        let (time_s, rest) = hms(time)?;
        let valid = parts.next().is_none()
            && rest.is_empty()
            && (1..=12).contains(&month)
            && (1..=5).contains(&week)
            && (0..=6).contains(&day);
        valid.then_some(Self {
            month,
            week,
            day,
            time_s: i64::from(time_s),
        })
    }

    /// The change in `year`, in seconds since the epoch on the clock it is given on.
    fn at(&self, year: i64) -> i64 {
        let first = days_from_civil(year, self.month, 1);
        let next_month = if self.month == 12 {
            days_from_civil(year + 1, 1, 1)
        } else {
            days_from_civil(year, self.month + 1, 1)
        };
        // 1970-01-01 was a Thursday: day 4 counted from Sunday.
        let mut day = first + (self.day - (first + 4)).rem_euclid(7) + 7 * (self.week - 1);
        while day >= next_month {
            day -= 7;
        }
        day * DAY_S + self.time_s
    }
}

/// The year `local_s` (seconds since the epoch) falls in.
fn year_of(local_s: i64) -> i64 {
    let days = local_s.div_euclid(DAY_S);
    let mut year = 1970 + days.div_euclid(366);
    while days_from_civil(year + 1, 1, 1) <= days {
        year += 1;
    }
    year
}

/// Howard Hinnant's `days_from_civil`: days since 1970-01-01 (proleptic Gregorian calendar).
const fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let month_from_march = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * month_from_march + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The tzdata release the table was built from, such as `2026f`.
pub fn version() -> &'static str {
    TABLE
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("# tzdata "))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use uniwrt_core::Timestamp;

    use super::{Local, TABLE, Weekday, days_from_civil, local, posix, standard_offset_s, version};

    /// `local` at a UTC date and time.
    fn at(tz: &str, (y, m, d): (i64, i64, i64), (h, min): (i64, i64)) -> (Weekday, u16, i32) {
        let secs = days_from_civil(y, m, d) * 86_400 + h * 3_600 + min * 60;
        let Local {
            day,
            minute,
            offset_s,
        } = local(tz, Timestamp::from_unix_secs(secs)).unwrap();
        (day, minute, offset_s)
    }

    #[test]
    fn local_time_follows_the_daylight_saving_rule() {
        let berlin = "CET-1CEST,M3.5.0,M10.5.0/3";
        // 2026-03-29 (a Sunday): the clocks go from 02:00 to 03:00 at 01:00 UTC.
        assert_eq!(
            at(berlin, (2026, 3, 29), (0, 59)),
            (Weekday::Sun, 119, 3_600)
        );
        assert_eq!(
            at(berlin, (2026, 3, 29), (1, 0)),
            (Weekday::Sun, 180, 7_200)
        );
        // 2026-10-25: back from 03:00 to 02:00 at 01:00 UTC.
        assert_eq!(
            at(berlin, (2026, 10, 25), (0, 59)),
            (Weekday::Sun, 179, 7_200)
        );
        assert_eq!(
            at(berlin, (2026, 10, 25), (1, 0)),
            (Weekday::Sun, 120, 3_600)
        );
        // The local day is the zone's, not UTC's.
        assert_eq!(
            at(berlin, (2026, 11, 11), (23, 30)),
            (Weekday::Thu, 30, 3_600)
        );
        let denver = "MST7MDT,M3.2.0,M11.1.0";
        assert_eq!(
            at(denver, (2026, 3, 8), (8, 59)),
            (Weekday::Sun, 119, -25_200)
        );
        assert_eq!(
            at(denver, (2026, 3, 8), (9, 0)),
            (Weekday::Sun, 180, -21_600)
        );
        assert_eq!(
            at(denver, (2026, 11, 1), (7, 59)),
            (Weekday::Sun, 119, -21_600)
        );
        assert_eq!(
            at(denver, (2026, 11, 1), (8, 0)),
            (Weekday::Sun, 60, -25_200)
        );
        assert_eq!(
            at(denver, (2026, 1, 1), (3, 0)),
            (Weekday::Wed, 1_200, -25_200)
        );
    }

    #[test]
    fn local_time_knows_the_unusual_rules() {
        // South of the equator the summer spans the new year.
        let sydney = "AEST-10AEDT,M10.1.0,M4.1.0/3";
        assert_eq!(at(sydney, (2026, 1, 15), (0, 0)).2, 39_600);
        assert_eq!(
            at(sydney, (2026, 4, 4), (15, 59)),
            (Weekday::Sun, 179, 39_600)
        );
        assert_eq!(
            at(sydney, (2026, 4, 4), (16, 0)),
            (Weekday::Sun, 120, 36_000)
        );
        assert_eq!(
            at(sydney, (2026, 10, 3), (16, 0)),
            (Weekday::Sun, 180, 39_600)
        );
        // Dublin's "daylight" time is its winter time, an hour behind.
        let dublin = "IST-1GMT0,M10.5.0,M3.5.0/1";
        assert_eq!(at(dublin, (2026, 7, 1), (12, 0)).2, 3_600);
        assert_eq!(at(dublin, (2026, 12, 1), (12, 0)).2, 0);
        // Times beyond a day: Jerusalem changes on the Friday before the last Sunday of March
        // (Thursday 26:00), Nuuk the evening before (-1).
        let jerusalem = "IST-2IDT,M3.4.4/26,M10.5.0";
        assert_eq!(at(jerusalem, (2026, 3, 26), (23, 59)).2, 7_200);
        assert_eq!(
            at(jerusalem, (2026, 3, 27), (0, 0)),
            (Weekday::Fri, 180, 10_800)
        );
        let nuuk = "<-02>2<-01>,M3.5.0/-1,M10.5.0/0";
        assert_eq!(at(nuuk, (2026, 3, 29), (0, 59)).2, -7_200);
        assert_eq!(at(nuuk, (2026, 3, 29), (1, 0)).2, -3_600);
        assert_eq!(at(nuuk, (2026, 10, 25), (1, 0)).2, -7_200);
        // No rule, and what doesn't parse.
        assert_eq!(
            at("<+0545>-5:45", (2026, 6, 1), (0, 0)),
            (Weekday::Mon, 345, 20_700)
        );
        assert_eq!(
            at("UTC0", (1969, 12, 31), (23, 59)),
            (Weekday::Wed, 1_439, 0)
        );
        let now = Timestamp::from_unix_secs(1_800_000_000);
        for bad in [
            "",
            "bad",
            "CET-1CEST",
            "CET-1CEST,J60,J300",
            "CET-1CEST,M13.1.0,M10.5.0",
        ] {
            assert_eq!(local(bad, now), None, "{bad}");
        }
        for line in TABLE.lines().filter(|line| !line.starts_with('#')) {
            let (zone, tz) = line.split_once('\t').unwrap();
            assert!(local(tz, now).is_some(), "{zone}: {tz}");
        }
    }

    #[test]
    fn standard_offsets_read_posix_signs_backwards() {
        assert_eq!(standard_offset_s("CET-1CEST,M3.5.0,M10.5.0/3"), Some(3600));
        assert_eq!(standard_offset_s("EST5EDT,M3.2.0,M11.1.0"), Some(-18_000));
        assert_eq!(standard_offset_s("UTC0"), Some(0));
        assert_eq!(standard_offset_s("<+0545>-5:45"), Some(20_700));
        assert_eq!(standard_offset_s("IST-5:30"), Some(19_800));
        assert_eq!(standard_offset_s("<-03>3"), Some(-10_800));
        assert_eq!(standard_offset_s("bad"), None);
        for line in TABLE.lines().filter(|line| !line.starts_with('#')) {
            let (zone, tz) = line.split_once('\t').unwrap();
            assert!(standard_offset_s(tz).is_some(), "{zone}: {tz}");
        }
    }

    #[test]
    fn zones_and_links_have_posix_strings() {
        assert_eq!(posix("Europe/Berlin"), Some("CET-1CEST,M3.5.0,M10.5.0/3"));
        assert_eq!(posix("America/Denver"), Some("MST7MDT,M3.2.0,M11.1.0"));
        assert_eq!(posix("US/Mountain"), posix("America/Denver"), "a link");
        assert_eq!(posix("UTC"), Some("UTC0"));
        assert_eq!(posix("Etc/GMT+5"), Some("<-05>5"));
        assert_eq!(posix("Mars/Olympus"), None);
        assert_eq!(posix(""), None);
        assert!(version().len() >= 5, "{}", version());
    }

    #[test]
    fn the_table_is_sorted_and_well_formed() {
        let rows: Vec<(&str, &str)> = TABLE
            .lines()
            .skip(1)
            .map(|line| line.split_once('\t').unwrap())
            .collect();
        assert!(rows.len() > 500);
        assert!(rows.windows(2).all(|w| w[0].0 < w[1].0));
        assert!(rows.iter().all(|(_, tz)| !tz.is_empty() && tz.is_ascii()));
    }
}
