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
    use super::{TABLE, posix, version};

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
