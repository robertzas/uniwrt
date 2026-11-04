//! The fixtures against what `uci` printed for them (`scripts/uci-conformance.sh` writes the
//! `.out` files and checks that each fixture is listed here).

use crate::{parse, parse_export};

macro_rules! fixtures {
    ($($path:literal),* $(,)?) => {
        &[$((
            $path,
            include_str!(concat!("../../fixtures/", $path)),
            include_str!(concat!("../../fixtures/", $path, ".out")),
        )),*]
    };
}

/// Each fixture: its path under `fixtures/`, its text and what `uci export` printed.
const FIXTURES: &[(&str, &str, &str)] = fixtures![
    "cases/abbreviations",
    "cases/anonymous",
    "cases/backslash",
    "cases/bare-config",
    "cases/bare-option",
    "cases/before-section",
    "cases/comments",
    "cases/continuation-indented",
    "cases/crlf",
    "cases/empty",
    "cases/empty-values",
    "cases/error-no-final-newline",
    "cases/invalid-command",
    "cases/merge-sections",
    "cases/multiline",
    "cases/no-final-newline",
    "cases/only-comments",
    "cases/option-list",
    "cases/option-name",
    "cases/package-invalid",
    "cases/package-statement",
    "cases/quoting",
    "cases/section-name",
    "cases/semicolon-after-quote",
    "cases/semicolon-double",
    "cases/semicolon-glued",
    "cases/semicolon-glued-command",
    "cases/semicolons",
    "cases/too-many-comment",
    "cases/too-many-config",
    "cases/too-many-option",
    "cases/trailing-empty-word",
    "cases/type-conflict",
    "cases/type-dotted",
    "cases/type-empty",
    "cases/type-invalid",
    "cases/type-quoted",
    "cases/unterminated-double",
    "cases/unterminated-eof",
    "cases/unterminated-single",
    "cases/whitespace",
    "openwrt/24.10/dhcp",
    "openwrt/24.10/dropbear",
    "openwrt/24.10/firewall",
    "openwrt/24.10/luci",
    "openwrt/24.10/network",
    "openwrt/24.10/rpcd",
    "openwrt/24.10/system",
    "openwrt/24.10/uhttpd",
    "openwrt/25.12/attendedsysupgrade",
    "openwrt/25.12/dhcp",
    "openwrt/25.12/dropbear",
    "openwrt/25.12/firewall",
    "openwrt/25.12/luci",
    "openwrt/25.12/network",
    "openwrt/25.12/rpcd",
    "openwrt/25.12/system",
    "openwrt/25.12/uhttpd",
    "openwrt/gateway/dhcp",
    "openwrt/gateway/firewall",
    "openwrt/gateway/network",
    "openwrt/gateway/wireless",
];

/// `uci`'s words for an error.
fn message(error: &crate::ParseError) -> String {
    format!(
        "uci: Parse error ({}) at line {}, byte {}\n",
        error.reason, error.line, error.byte
    )
}

#[test]
fn reads_and_writes_as_uci() {
    for (path, text, expected) in FIXTURES {
        let name = path.rsplit('/').next().unwrap_or(path);
        let got = parse(name, text).map_or_else(|e| message(&e), |p| p.to_export());
        assert_eq!(got, *expected, "{path}");
    }
}

#[test]
fn what_parses_round_trips() {
    for (path, text, expected) in FIXTURES {
        let name = path.rsplit('/').next().unwrap_or(path);
        let Ok(package) = parse(name, text) else {
            continue;
        };
        assert_eq!(package.validate(), Ok(()), "{path}");
        assert_eq!(
            parse(name, &package.to_file()).as_ref(),
            Ok(&package),
            "{path}"
        );
        assert_eq!(parse_export(expected), Ok(vec![package]), "{path}");
    }
}

#[test]
fn real_configs_are_whole() {
    let (_, text, _) = FIXTURES
        .iter()
        .find(|(p, ..)| *p == "openwrt/25.12/firewall")
        .unwrap();
    let firewall = parse("firewall", text).unwrap();
    assert_eq!(firewall.sections_of("rule").count(), 9);
    let zone = firewall.sections_of("zone").nth(1).unwrap();
    assert_eq!(zone.option("name"), Some("wan"));
    assert_eq!(zone.get("network").unwrap().items(), ["wan", "wan6"]);
    let (_, text, _) = FIXTURES
        .iter()
        .find(|(p, ..)| *p == "openwrt/gateway/wireless")
        .unwrap();
    let wireless = parse("wireless", text).unwrap();
    let guest = wireless.section("uw_guest_2g").unwrap();
    assert_eq!(guest.option("key"), Some(r#"it's a "guest" key; #1"#));
}
