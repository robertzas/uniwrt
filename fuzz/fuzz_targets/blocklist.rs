//! DNS filtering's lists (threat-model.md §6): each line read as hosts, plain, adblock or
//! dnsmasq lines; every name found is already in its normal form, and the compiled file holds
//! nothing but its header and `server=/<name>/` lines (blocked) and `server=/<name>/#` ones
//! (allowed), so no list can inject a dnsmasq directive.
#![no_main]

use libfuzzer_sys::fuzz_target;
use uniwrt_dns::{compile, parse};

fuzz_target!(|data: &[u8]| {
    let mut names = Vec::new();
    for line in data.split(|&byte| byte == b'\n') {
        parse::line(line, &mut names);
    }
    for name in &names {
        assert_eq!(parse::domain(name).as_deref(), Some(name.as_str()));
    }
    let allow = names.iter().step_by(3).cloned().collect::<Vec<_>>();
    let compiled = compile(&[data], &allow, &[], 1_000);
    let text = std::str::from_utf8(&compiled.text).expect("the compiled file is UTF-8");
    for line in text.lines().skip(1) {
        let name = line
            .strip_prefix("server=/")
            .and_then(|rest| rest.strip_suffix("/#").or_else(|| rest.strip_suffix('/')))
            .unwrap_or_else(|| panic!("not a server line: {line:?}"));
        assert_eq!(parse::domain(name).as_deref(), Some(name), "{line:?}");
    }
});
