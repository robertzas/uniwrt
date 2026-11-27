//! `POST /api/v1/ztp/hello` (threat-model.md §2, unauthenticated): the body read as the
//! API reads it, then checked as the inbox checks it.
#![no_main]

use std::net::{IpAddr, Ipv4Addr};

use libfuzzer_sys::fuzz_target;
use uniwrt_core::Timestamp;
use uniwrt_model::ztp::ZtpHello;

fuzz_target!(|data: &[u8]| {
    if let Ok(hello) = serde_json::from_slice::<ZtpHello>(data) {
        let _ = uniwrt_node::ztp::pending(
            &hello,
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 20)),
            Timestamp::from_unix_secs(1_764_000_000),
        );
    }
});
