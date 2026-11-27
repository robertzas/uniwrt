//! The syslog receiver (threat-model.md §3): a TCP stream framed as RFC 6587 frames it, in
//! chunks of a size the input's length picks, and the whole input as one UDP datagram; each
//! message parsed (RFC 3164 or 5424), annotated by the embedded patterns and read as dnsmasq's
//! query log.
#![no_main]

use libfuzzer_sys::fuzz_target;
use uniwrt_core::Timestamp;
use uniwrt_model::dns_queries;
use uniwrt_syslog::{Framer, Patterns, parse};

fuzz_target!(|data: &[u8]| {
    let mut framer = Framer::new();
    let frames: Vec<Vec<u8>> = data
        .chunks(data.len() % 61 + 1)
        .flat_map(|part| framer.push(part))
        .collect();
    let received = Timestamp::from_unix_secs(1_764_000_000);
    for message in frames.iter().map(Vec::as_slice).chain([data]) {
        if let Some(message) = parse(message, received) {
            let _ = Patterns::embedded().annotate(&message.entry);
            let _ = dns_queries::parse(message.entry.program.as_deref(), &message.entry.message);
        }
    }
});
