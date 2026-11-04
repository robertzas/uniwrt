//! UCI identifiers, as libuci checks them (`uci_validate_str` in util.c, ASCII in the C locale).

/// A section or option name: `[A-Za-z0-9_]+`.
pub fn is_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// A package name, as a `package` statement may give it: `[A-Za-z0-9_-]+`. libuci loads a
/// file by a laxer rule, but the `uci` CLI cannot address a package whose name holds a `.`.
pub fn is_package(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// A section type: printable ASCII without spaces, such as `wifi-iface`.
pub fn is_type(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_graphic())
}

#[cfg(test)]
mod tests {
    use super::{is_name, is_package, is_type};

    #[test]
    fn names() {
        assert!(is_name("wifi_iface") && is_name("Up2") && !is_name("wifi-iface"));
        assert!(!is_name("") && !is_name("a.b") && !is_name("ü"));
        assert!(is_package("luci-ssl") && !is_package("a.b") && !is_package(""));
        assert!(is_type("wifi-iface") && is_type("a.b") && !is_type("a b") && !is_type(""));
        assert!(!is_type("é"));
    }
}
