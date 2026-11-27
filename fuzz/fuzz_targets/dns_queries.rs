//! dnsmasq's query log (threat-model.md §3): each line read as the statistics read it, and the
//! queries paired with their answers per process (two processes, the lines alternating).
#![no_main]

use std::time::Duration;

use libfuzzer_sys::fuzz_target;
use uniwrt_core::Timestamp;
use uniwrt_model::dns_queries::{Pairing, parse};

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let mut pairing = Pairing::new("lan");
    let mut at = Timestamp::from_unix_secs(1_764_000_000);
    for (n, line) in text.lines().enumerate() {
        if let Some(line) = parse(Some("dnsmasq"), line) {
            let _ = pairing.line(&(n % 2), at, line);
        }
        at = at.saturating_add(Duration::from_millis(100));
    }
    let _ = pairing.expire(at.saturating_add(Duration::from_secs(10)));
});
