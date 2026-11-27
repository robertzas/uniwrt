//! A node's `sysupgrade -b` backup (threat-model.md §4): its entries unpacked in memory,
//! its configuration files and its content digest.
#![no_main]

use libfuzzer_sys::fuzz_target;
use uniwrt_node::archive;

fuzz_target!(|data: &[u8]| {
    if let Ok(entries) = archive::entries(data) {
        let _ = archive::config_files(&entries);
    }
    let _ = archive::content_digest(data);
});
