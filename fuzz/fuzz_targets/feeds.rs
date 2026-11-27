//! The feed mirror's readers (threat-model.md §2 and Outbound HTTPS): the input as opkg's
//! `Packages.gz`, its text, apk's `packages.adb` and a feed's URL.
#![no_main]

use libfuzzer_sys::fuzz_target;
use uniwrt_feeds::{feed::Feed, index};

fuzz_target!(|data: &[u8]| {
    let _ = index::opkg::parse(data);
    let _ = index::adb::parse(data);
    let text = String::from_utf8_lossy(data);
    let _ = index::opkg::parse_text(&text);
    let _ = Feed::parse(&text);
});
