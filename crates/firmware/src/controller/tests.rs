use serde_json::Value;

use super::{CONTROLLER_REPO, Version, binary_name};
use crate::{
    Error, GitHub,
    sigstore::{Provenance, Subject, Untrusted},
    testing::FakeReleases,
};

const TARGET: &str = "x86_64-unknown-linux-musl";

/// Accepts the fake's attestations of the subject's own digest.
fn accept(bundle: &[u8], subject: Subject<'_>) -> Result<Provenance, Untrusted> {
    let bundle: Value = serde_json::from_slice(bundle).unwrap();
    let hex = data_encoding::HEXLOWER.encode(subject.sha256);
    if bundle["fake"] == hex {
        Ok(Provenance {
            commit: None,
            logged_at: 1,
        })
    } else {
        Err(Untrusted("another artifact".to_owned()))
    }
}

fn refuse(_: &[u8], _: Subject<'_>) -> Result<Provenance, Untrusted> {
    Err(Untrusted("the certificate wasn't issued to CI".to_owned()))
}

#[test]
fn versions_are_release_versions_only() {
    assert_eq!("0.108.0".parse(), Ok(Version(0, 108, 0)));
    assert_eq!("v1.2.3".parse(), Ok(Version(1, 2, 3)));
    for bad in [
        "0.0.0-dev.3+abc",
        "1.2",
        "1.2.3.4",
        "1..3",
        "v",
        "1.2.x",
        "+1.2.3",
    ] {
        assert_eq!(bad.parse::<Version>(), Err(()), "{bad}");
    }
    assert!(Version(0, 108, 0) > Version(0, 99, 9));
    assert_eq!(Version(0, 108, 0).to_string(), "0.108.0");
    assert_eq!(binary_name(Version(1, 0, 2)), "uniwrt-1.0.2");
}

#[tokio::test]
async fn the_newest_release_for_the_target_is_found_and_staged() {
    let fake = FakeReleases::start().await;
    fake.publish("0.107.0", TARGET, b"old");
    fake.publish("0.109.0", TARGET, b"newest binary");
    fake.publish("0.110.0", "aarch64-unknown-linux-musl", b"other target");
    let github = GitHub::new(fake.url(), "uniwrt/test").unwrap();
    let releases = github.releases(CONTROLLER_REPO, None).await.unwrap();
    let release = releases.controller(TARGET).unwrap();
    assert_eq!(release.version, Version(0, 109, 0));
    assert_eq!(
        release.tarball,
        "uniwrt-0.109.0-x86_64-unknown-linux-musl.tar.gz"
    );
    assert!(releases.controller("mips-unknown-linux-musl").is_none());

    let dir = tempfile::tempdir().unwrap();
    let staged = github
        .stage_controller(&release, TARGET, dir.path(), accept)
        .await
        .unwrap();
    assert_eq!(staged.version, Version(0, 109, 0));
    assert_eq!(staged.path, dir.path().join("uniwrt-0.109.0"));
    assert_eq!(std::fs::read(&staged.path).unwrap(), b"newest binary");
    let left: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(left, ["uniwrt-0.109.0"]);
}

#[tokio::test]
async fn a_refused_attestation_or_a_wrong_digest_stages_nothing() {
    let fake = FakeReleases::start().await;
    fake.publish("0.109.0", TARGET, b"binary");
    let github = GitHub::new(fake.url(), "uniwrt/test").unwrap();
    let release = github
        .releases(CONTROLLER_REPO, None)
        .await
        .unwrap()
        .controller(TARGET)
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let err = github
        .stage_controller(&release, TARGET, dir.path(), refuse)
        .await
        .unwrap_err();
    assert!(
        matches!(&err, Error::Untrusted(why) if why.contains("issued to CI")),
        "{err}"
    );
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);

    fake.misdigest("0.109.0");
    github.forget();
    let release = github
        .releases(CONTROLLER_REPO, None)
        .await
        .unwrap()
        .controller(TARGET)
        .unwrap();
    let err = github
        .stage_controller(&release, TARGET, dir.path(), accept)
        .await
        .unwrap_err();
    assert!(
        matches!(&err, Error::Untrusted(why) if why.contains("GitHub lists")),
        "{err}"
    );
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[tokio::test]
async fn a_tarball_without_the_binary_for_the_target_is_refused() {
    let fake = FakeReleases::start().await;
    fake.publish("0.109.0", TARGET, b"binary");
    let github = GitHub::new(fake.url(), "uniwrt/test").unwrap();
    let mut release = github
        .releases(CONTROLLER_REPO, None)
        .await
        .unwrap()
        .controller(TARGET)
        .unwrap();
    // The tarball of another version names another directory.
    release.version = Version(0, 109, 1);
    let dir = tempfile::tempdir().unwrap();
    let err = github
        .stage_controller(&release, TARGET, dir.path(), accept)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("holds no"), "{err}");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}
