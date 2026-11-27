//! UCI (threat-model.md §4, Imports): the input as a file `uci` loads and as `uci export`'s
//! output. Every package parsed differs from itself in nothing, and one that validates reads
//! back as itself from `uci commit`'s and `uci export`'s text (the crate's contract).
#![no_main]

use libfuzzer_sys::fuzz_target;
use uniwrt_uci::{Package, diff, parse, parse_export};

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    if let Ok(package) = parse("network", text) {
        check(&package);
    }
    if let Ok(packages) = parse_export(text) {
        packages.iter().for_each(check);
    }
});

fn check(package: &Package) {
    assert!(
        diff(package, package).is_empty(),
        "a package differs from itself"
    );
    if package.validate().is_ok() {
        assert_eq!(
            parse(&package.name, &package.to_file()).as_ref(),
            Ok(package)
        );
        assert_eq!(
            parse_export(&package.to_export()),
            Ok(vec![package.clone()])
        );
    }
}
