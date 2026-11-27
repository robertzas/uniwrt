//! Entry points of the fuzz targets (`fuzz/`; M9.3b, D-150), behind the `fuzzing` feature: the
//! readers of what ASU, OpenWrt's download server and GitHub answer, and of an uploaded image's
//! metadata, which the crate doesn't export. They return nothing: a panic, a hang or an
//! allocation past libFuzzer's limit is the finding.

use crate::sigstore::{Identity, Subject, TrustedRoot, verify};

/// `body` read as each answer the catalog, a build, the device and package indexes and GitHub
/// give; GitHub's releases searched for the controller's and an x86 device's offers.
pub fn answers(body: &[u8]) {
    let _ = crate::catalog::overview(body);
    let _ = crate::catalog::profiles(body);
    let _ = crate::catalog::profile_info(body);
    let _ = crate::build::answer("https://asu.invalid", None, 200, body);
    let _ = crate::build::answer("https://asu.invalid", Some("0123abcd"), 500, body);
    let _ = crate::device::index(body);
    if let Ok(target) = crate::device::target(body)
        && let Some(device) = target.device("generic")
    {
        let _ = target.packages(device);
    }
    if let Ok(releases) = crate::github::parse("owner/repo", body) {
        let _ = releases.controller("x86_64-unknown-linux-musl");
    }
}

/// `bytes` as an uploaded image named `name`: its first 4 KiB and its last 64 KiB.
pub fn image(name: &str, bytes: &[u8]) {
    let head = &bytes[..bytes.len().min(4 << 10)];
    let tail = &bytes[bytes.len().saturating_sub(64 << 10)..];
    let _ = crate::image::inspect(name, head, tail);
}

/// `body` as GitHub's attestations of a tarball, each bundle verified against the embedded
/// trust root for uniwrt's workflow; and `body` itself as a bundle.
pub fn attestations(body: &[u8]) {
    let Ok(root) = TrustedRoot::embedded() else {
        return;
    };
    let subject = Subject {
        name: "uniwrt-x86_64-unknown-linux-musl.tar.gz",
        sha256: &[0; 32],
    };
    let identity = Identity {
        workflow: "https://github.com/robertzas/uniwrt/.github/workflows/ci.yml@refs/heads/main",
        issuer: "https://token.actions.githubusercontent.com",
    };
    for bundle in crate::controller::bundles(body).unwrap_or_default() {
        let _ = verify(&bundle, subject, identity, &root);
    }
    let _ = verify(body, subject, identity, &root);
}
