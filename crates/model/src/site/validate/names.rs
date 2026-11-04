//! The names a document holds: ids, host and DNS names. UCI identifiers follow `uniwrt_uci`.

/// A network or WLAN id: `[a-z][a-z0-9_]{0,11}`, short enough for the UCI and device names
/// built on it (`uw_<wlan>_5g2`).
pub fn is_id(id: &str) -> bool {
    let mut bytes = id.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_lowercase())
        && id.len() <= 12
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// A host name: one DNS label (RFC 1123).
pub fn is_hostname(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 63
        && !name.starts_with('-')
        && !name.ends_with('-')
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// A DNS name: dot-separated host names, up to 253 bytes.
pub(super) fn is_dns_name(name: &str) -> bool {
    name.len() <= 253 && name.split('.').all(is_hostname)
}

#[cfg(test)]
mod tests {
    use super::{is_dns_name, is_hostname, is_id};

    #[test]
    fn names() {
        assert!(is_id("lan") && is_id("iot_2") && is_id("abcdefghijkl"));
        assert!(!is_id("") && !is_id("2g") && !is_id("Lan") && !is_id("abcdefghijklm"));
        assert!(!is_id("lan-2"));
        assert!(is_hostname("ap-garden") && !is_hostname("-ap") && !is_hostname("a_b"));
        assert!(is_dns_name("home.arpa") && !is_dns_name("home..arpa") && !is_dns_name(""));
    }
}
