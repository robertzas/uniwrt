//! Time zones: the IANA names a site may use and the POSIX TZ string OpenWrt's
//! `system.timezone` takes for each (`data/tz.tsv`, built from tzdata by `just tz`).

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
/// is 3600, `EST5EDT,…` is -18000); `None` when it doesn't parse. Daylight saving time is left
/// out: an hour off at most, for what only needs "at night".
pub fn standard_offset_s(posix: &str) -> Option<i32> {
    let rest = match posix.strip_prefix('<') {
        Some(quoted) => &quoted[quoted.find('>')? + 1..],
        None => posix.trim_start_matches(|c: char| c.is_ascii_alphabetic()),
    };
    let (sign, rest) = match rest.as_bytes().first()? {
        b'-' => (1, &rest[1..]),
        b'+' => (-1, &rest[1..]),
        _ => (-1, rest),
    };
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == ':'))
        .unwrap_or(rest.len());
    let mut parts = rest[..end].split(':').map(str::parse::<i32>);
    let hours = parts.next()?.ok()?;
    let minutes = parts.next().transpose().ok()?.unwrap_or(0);
    let seconds = parts.next().transpose().ok()?.unwrap_or(0);
    (hours <= 24).then_some(sign * (hours * 3600 + minutes * 60 + seconds))
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
    use super::{TABLE, posix, standard_offset_s, version};

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
