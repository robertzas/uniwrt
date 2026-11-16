use serde_json::json;

use super::*;
use crate::testing::{FakeGitHub, SHA256, TOKEN};

const MT6000: Device<'static> = Device {
    target: "mediatek/filogic",
    board: "glinet,gl-mt6000",
    profile: Some("glinet_gl-mt6000"),
};

fn github(fake: &FakeGitHub) -> GitHub {
    GitHub::new(fake.url(), "uniwrt/test").unwrap()
}

fn tags(offers: &[GitHubRelease]) -> Vec<&str> {
    offers.iter().map(|release| release.tag.as_str()).collect()
}

#[tokio::test]
async fn a_node_gets_the_releases_with_an_image_for_it() {
    let fake = FakeGitHub::start().await;
    let releases = github(&fake).releases("owner/images", None).await.unwrap();
    // v1's image has no SHA-256, v3 is a prerelease, the draft is never offered.
    let (offers, note) = releases.offers(MT6000, false);
    assert_eq!((tags(&offers), note), (vec!["v2"], None));
    let image = &offers[0].images[0];
    assert_eq!(image.kind, "sysupgrade");
    assert_eq!(image.filesystem.as_deref(), Some("squashfs"));
    assert_eq!(image.sha256, SHA256);
    assert_eq!(image.size_bytes, Some(9_437_184));
    assert!(
        image
            .url
            .ends_with(format!("/download/v2/{}", image.name).as_str())
    );
    assert_eq!(offers[0].images.len(), 1, "the factory image isn't offered");
    assert_eq!(offers[0].name, "Build v2");
    assert!(offers[0].notes.ends_with("/releases/tag/v2"));
    assert_eq!(
        offers[0].published_at,
        Timestamp::parse_utc("2026-11-01T08:00:00Z")
    );

    let (offers, _) = releases.offers(MT6000, true);
    assert_eq!(tags(&offers), ["v3", "v2"]);
    assert!(offers[0].prerelease);

    let x86 = Device {
        target: "x86/64",
        board: "pc-engines-apu2",
        profile: None,
    };
    let (offers, _) = releases.offers(x86, false);
    assert_eq!(tags(&offers), ["v2"]);
    assert_eq!(offers[0].images[0].kind, "combined-efi");

    let other = Device {
        board: "linksys,e8450-ubi",
        profile: Some("linksys_e8450-ubi"),
        ..MT6000
    };
    let (offers, note) = releases.offers(other, true);
    assert_eq!(offers, []);
    assert_eq!(
        note.as_deref(),
        Some("No release of owner/images has a sysupgrade image named for linksys_e8450-ubi")
    );
}

#[tokio::test]
async fn releases_are_kept_a_while_and_forgotten_on_demand() {
    let fake = FakeGitHub::start().await;
    let github = github(&fake);
    github.releases("owner/images", None).await.unwrap();
    github.releases("owner/images", None).await.unwrap();
    assert_eq!(fake.requests().len(), 1);
    github.forget();
    github.releases("owner/images", None).await.unwrap();
    assert_eq!(fake.requests().len(), 2);
    assert_eq!(fake.requests()[0], ("owner/images".to_owned(), None));
}

#[tokio::test]
async fn a_private_repo_needs_the_token() {
    let fake = FakeGitHub::start().await;
    let github = github(&fake);
    let hidden = github.releases("owner/private", None).await;
    assert!(
        matches!(&hidden, Err(Error::NotFound(why)) if why.contains("a private one needs a token")),
        "{hidden:?}"
    );
    let releases = github.releases("owner/private", Some(TOKEN)).await.unwrap();
    assert_eq!(tags(&releases.offers(MT6000, false).0), ["p1"]);
    assert_eq!(
        fake.requests()[1].1.as_deref(),
        Some(format!("Bearer {TOKEN}").as_str())
    );
    // A new token comes with forgetting what the old one read.
    github.forget();
    let wrong = github.releases("owner/private", Some("other")).await;
    assert!(matches!(&wrong, Err(Error::NotFound(why)) if why.contains("the token can read")));
}

#[tokio::test]
async fn a_spent_rate_limit_says_until_when() {
    let fake = FakeGitHub::start().await;
    let err = github(&fake)
        .releases("owner/limited", None)
        .await
        .unwrap_err()
        .to_string();
    assert_eq!(
        err,
        "GitHub's rate limit is spent until 2026-11-06T21:20:00Z; a token raises it"
    );
}

#[test]
fn unhashed_images_are_named_as_the_reason() {
    let body = json!([{
        "tag_name": "v1", "html_url": "https://github.com/o/r/releases/tag/v1",
        "published_at": "2026-01-01T00:00:00Z",
        "assets": [{"name": "glinet_gl-mt6000-sysupgrade.bin", "size": 1,
                    "browser_download_url": "https://github.com/o/r/a.bin"}]
    }]);
    let releases = parse("o/r", body.to_string().as_bytes()).unwrap();
    let (offers, note) = releases.offers(MT6000, false);
    assert_eq!(offers, []);
    assert!(note.unwrap().contains("have no SHA-256 on GitHub"));
}

#[test]
fn five_releases_are_offered_at_most_and_bad_links_are_dropped() {
    let mut list: Vec<_> = (1..=7)
        .map(|n| {
            json!({
                "tag_name": format!("v{n}"), "html_url": format!("https://github.com/o/r/releases/tag/v{n}"),
                "published_at": format!("2026-01-0{n}T00:00:00Z"),
                "assets": [{"name": "glinet_gl-mt6000-sysupgrade.bin", "size": 1,
                            "digest": format!("sha256:{SHA256}"),
                            "browser_download_url": format!("https://github.com/o/r/v{n}.bin")}]
            })
        })
        .collect();
    list.push(json!({
        "tag_name": "v9", "html_url": "javascript:alert(1)", "published_at": "2026-01-09T00:00:00Z",
        "assets": []
    }));
    let releases = parse("o/r", serde_json::to_vec(&list).unwrap().as_slice()).unwrap();
    let (offers, _) = releases.offers(MT6000, false);
    assert_eq!(tags(&offers), ["v7", "v6", "v5", "v4", "v3"]);
}

#[test]
fn repos_are_read_as_typed_or_as_links() {
    for (typed, repo) in [
        ("owner/images", Some("owner/images")),
        (
            " https://github.com/Owner/my.images/ ",
            Some("Owner/my.images"),
        ),
        ("github.com/owner/images.git", Some("owner/images")),
        (
            "https://github.com/owner/images/releases",
            Some("owner/images"),
        ),
        ("https://github.com/owner/images/tree/main", None),
        ("owner", None),
        ("-owner/images", None),
        ("owner/..", None),
        ("owner/im ages", None),
        ("https://gitlab.com/owner/images", None),
    ] {
        assert_eq!(parse_repo(typed).as_deref(), repo, "{typed}");
    }
    assert!(valid_token("github_pat_11ABC"));
    assert!(!valid_token("with space"));
    assert!(!valid_token(""));
}
