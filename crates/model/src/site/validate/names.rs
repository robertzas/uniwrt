//! The names a document holds: ids, host and DNS names, UCI identifiers.

/// A network or WLAN id: `[a-z][a-z0-9_]{0,11}`, short enough for `br-<id>` to fit a Linux
/// interface name.
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

/// A UCI section or option name, as libuci checks it: `[A-Za-z0-9_]+`.
pub fn is_uci_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// A UCI package name: `[A-Za-z0-9_-]+`.
pub fn is_uci_package(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// A UCI section type: printable ASCII without spaces, such as `wifi-iface`.
pub fn is_uci_type(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64 && name.bytes().all(|b| b.is_ascii_graphic())
}

#[cfg(test)]
mod tests {
    use super::{is_dns_name, is_hostname, is_id, is_uci_name, is_uci_package, is_uci_type};

    #[test]
    fn names() {
        assert!(is_id("lan") && is_id("iot_2") && is_id("abcdefghijkl"));
        assert!(!is_id("") && !is_id("2g") && !is_id("Lan") && !is_id("abcdefghijklm"));
        assert!(!is_id("lan-2"));
        assert!(is_hostname("ap-garden") && !is_hostname("-ap") && !is_hostname("a_b"));
        assert!(is_dns_name("home.arpa") && !is_dns_name("home..arpa") && !is_dns_name(""));
        assert!(is_uci_name("wifi_iface") && !is_uci_name("wifi-iface") && !is_uci_name(""));
        assert!(is_uci_package("luci-ssl") && !is_uci_package("a.b"));
        assert!(is_uci_type("wifi-iface") && !is_uci_type("a b") && !is_uci_type(""));
    }
}
