//! Dependency-free UTC date formatting, shared by the build script and the crate's unit tests.

/// Formats seconds since the Unix epoch as RFC 3339 UTC, e.g. `2026-10-22T10:45:00Z`.
pub(crate) fn rfc3339_utc(unix_secs: u64) -> String {
    let (year, month, day) = civil_from_days(unix_secs / 86_400);
    let secs = unix_secs % 86_400;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        secs / 3_600,
        secs / 60 % 60,
        secs % 60
    )
}

/// Howard Hinnant's `civil_from_days` for days since 1970-01-01 (proleptic Gregorian calendar).
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    let shifted = days + 719_468; // days since 0000-03-01
    let era = shifted / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
    let month = if month_from_march < 10 {
        month_from_march + 3
    } else {
        month_from_march - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::rfc3339_utc;

    #[test]
    fn formats_reference_instants() {
        // Expected values come from `date -u -d @<secs> +%Y-%m-%dT%H:%M:%SZ`.
        for (secs, expected) in [
            (0, "1970-01-01T00:00:00Z"),
            (951_782_400, "2000-02-29T00:00:00Z"),
            (951_868_800, "2000-03-01T00:00:00Z"),
            (1_709_251_199, "2024-02-29T23:59:59Z"),
            (1_792_622_700, "2026-10-21T22:45:00Z"),
            (4_102_444_799, "2099-12-31T23:59:59Z"),
        ] {
            assert_eq!(rfc3339_utc(secs), expected, "{secs}");
        }
    }
}
