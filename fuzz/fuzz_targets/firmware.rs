//! Outbound answers (threat-model.md §6): the input as each JSON answer of ASU, OpenWrt's
//! download server and GitHub (releases, attestations: each bundle verified against the
//! embedded Sigstore root), as an uploaded image, and as a trust root.
#![no_main]

use libfuzzer_sys::fuzz_target;
use uniwrt_firmware::{fuzzing, sigstore::TrustedRoot};

fuzz_target!(|data: &[u8]| {
    fuzzing::answers(data);
    fuzzing::image(
        "openwrt-24.10.4-x86-64-generic-squashfs-combined.img.gz",
        data,
    );
    fuzzing::attestations(data);
    let _ = TrustedRoot::parse(data);
});
