//! GitHub sources through the router (D-122), on the simulated fleet against the fake ASU and
//! the fake GitHub: the site's repo, a node's own source, the token, and the GitHub plan.

use axum::http::{Method, StatusCode};
use serde_json::{Value, json};
use uniwrt_firmware::{
    Asu, GitHub,
    testing::{FakeAsu, FakeGitHub, SHA256, TOKEN},
};

use crate::{
    firmware_tests::settled,
    testing::{Demo, json as body, post, request},
};

async fn start() -> (Demo, FakeAsu, FakeGitHub) {
    let (asu, github) = (FakeAsu::start().await, FakeGitHub::start().await);
    let demo = Demo::start_firmware(
        false,
        Some(Asu::new(asu.url(), "uniwrt/test").unwrap()),
        Some(GitHub::new(github.url(), "uniwrt/test").unwrap()),
    )
    .await;
    (demo, asu, github)
}

async fn put(demo: &Demo, uri: &str, body: &Value) -> (StatusCode, Value) {
    let res = demo
        .send(request(Method::PUT, uri, Some(body), Some(&demo.cookie)))
        .await;
    let status = res.status();
    (status, self::body(res).await)
}

async fn site(demo: &Demo, repo: &str, token: Value) -> (StatusCode, Value) {
    let sources = json!({"repo": repo, "prereleases": false, "token": token});
    put(demo, "/v1/firmware/sources", &sources).await
}

fn tags(node: &Value) -> Vec<&str> {
    node["releases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|release| release["tag"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn the_sites_repo_offers_its_releases_and_a_node_can_keep_openwrt() {
    let (demo, _asu, _github) = start().await;
    let before = settled(&demo).await;
    assert_eq!(
        before["sources"],
        json!({"repo": null, "prereleases": false, "token": false})
    );
    assert_eq!(before["nodes"][0]["source"], json!({"kind": "site"}));

    let (status, saved) = site(
        &demo,
        "https://github.com/owner/images",
        json!({"action": "keep"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["repo"], "owner/images");
    let catalog = settled(&demo).await;
    for node in catalog["nodes"].as_array().unwrap() {
        assert_eq!(node["repo"], "owner/images");
        assert_eq!(node["offers"], json!([]));
        assert_eq!(tags(node), ["v2"], "{node}");
        let image = &node["releases"][0]["images"][0];
        assert_eq!(image["sha256"], SHA256);
        let profile = node["board"].as_str().unwrap().replace(',', "_");
        assert!(image["name"].as_str().unwrap().contains(&profile));
    }

    // One node goes back to OpenWrt's releases, built by ASU.
    let older = catalog["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["version"] == "24.10.8")
        .expect("a node on 24.10");
    let id = older["node"].as_i64().unwrap();
    let uri = format!("/v1/firmware/nodes/{id}/source");
    let (status, source) = put(&demo, &uri, &json!({"kind": "asu"})).await;
    assert_eq!((status, source), (StatusCode::OK, json!({"kind": "asu"})));
    let catalog = settled(&demo).await;
    let node = catalog["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["node"] == id)
        .unwrap();
    assert_eq!(node["source"], json!({"kind": "asu"}));
    assert_eq!(node["repo"], Value::Null);
    assert_eq!(node["releases"], json!([]));
    assert_eq!(node["offers"][0]["release"], "25.12.6");

    // Back to the site's: the entry is dropped.
    let (status, _) = put(&demo, &uri, &json!({"kind": "site"})).await;
    assert_eq!(status, StatusCode::OK);
    let catalog = settled(&demo).await;
    assert!(
        catalog["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|node| node["source"] == json!({"kind": "site"}))
    );
}

#[tokio::test]
async fn a_github_plan_lists_the_releases_images_and_reads_nothing() {
    let (demo, _asu, _github) = start().await;
    let catalog = settled(&demo).await;
    let id = catalog["nodes"][0]["node"].as_i64().unwrap();
    let plan = |release: &str| {
        post(
            &format!("/v1/firmware/nodes/{id}/plan"),
            &json!({"source": "github", "release": release}),
            Some(&demo.cookie),
        )
    };
    // Without a repo, the node takes OpenWrt's releases.
    assert_eq!(demo.send(plan("v2")).await.status(), StatusCode::CONFLICT);

    site(&demo, "owner/images", json!({"action": "keep"})).await;
    let res = demo.send(plan("v2")).await;
    assert_eq!(res.status(), StatusCode::OK);
    let found = body(res).await;
    assert_eq!(found["source"], "github");
    assert_eq!(found["repo"], "owner/images");
    assert_eq!(found["release"], "v2");
    assert_eq!(found["images"][0]["sha256"], SHA256);
    assert_eq!(found["image_bytes"], 9_437_184);
    assert_eq!(found["packages_at"], Value::Null);
    assert_eq!(found["packages"], json!([]));
    // v1's image has no SHA-256, v3 is a prerelease (not offered), and drafts never are.
    for tag in ["v1", "v3", "draft", "nope"] {
        assert_eq!(
            demo.send(plan(tag)).await.status(),
            StatusCode::NOT_FOUND,
            "{tag}"
        );
    }
}

#[tokio::test]
async fn a_private_repo_takes_a_token_kept_in_the_vault() {
    let (demo, _asu, github) = start().await;
    settled(&demo).await;
    let (status, refused) = site(&demo, "owner/private", json!({"action": "keep"})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        refused["detail"]
            .as_str()
            .unwrap()
            .contains("needs a token")
    );

    let set = json!({"action": "set", "token": TOKEN});
    let (status, saved) = site(&demo, "owner/private", set).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["token"], true);
    let catalog = settled(&demo).await;
    assert_eq!(catalog["sources"]["token"], true);
    assert_eq!(tags(&catalog["nodes"][0]), ["p1"]);
    assert!(
        github
            .requests()
            .iter()
            .any(|(_, auth)| auth.as_deref() == Some(&format!("Bearer {TOKEN}")))
    );

    // Back to OpenWrt's releases, the token removed.
    let res = demo
        .send(request(
            Method::PUT,
            "/v1/firmware/sources",
            Some(&json!({"repo": null, "prereleases": true, "token": {"action": "clear"}})),
            Some(&demo.cookie),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        body(res).await,
        json!({"repo": null, "prereleases": true, "token": false})
    );
    assert_eq!(settled(&demo).await["nodes"][0]["repo"], Value::Null);
}

#[tokio::test]
async fn bad_repos_and_tokens_are_refused() {
    let (demo, _asu, _github) = start().await;
    let catalog = settled(&demo).await;
    let id = catalog["nodes"][0]["node"].as_i64().unwrap();
    for repo in ["owner", "https://gitlab.com/a/b"] {
        let (status, _) = site(&demo, repo, json!({"action": "keep"})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{repo}");
    }
    let (status, _) = site(
        &demo,
        "owner/images",
        json!({"action": "set", "token": "a b"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = site(&demo, "owner/missing", json!({"action": "keep"})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // GitHub's rate limit doesn't block a save: the catalog says why nothing is offered.
    let (status, _) = site(&demo, "owner/limited", json!({"action": "keep"})).await;
    assert_eq!(status, StatusCode::OK);
    let note = settled(&demo).await["nodes"][0]["note"].clone();
    assert!(note.as_str().unwrap().contains("rate limit"), "{note}");

    let uri = format!("/v1/firmware/nodes/{id}/source");
    let (status, _) = put(
        &demo,
        &uri,
        &json!({"kind": "github", "repo": "owner/missing"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, saved) = put(
        &demo,
        &uri,
        &json!({"kind": "github", "repo": "github.com/owner/images"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved, json!({"kind": "github", "repo": "owner/images"}));
    let (status, _) = put(
        &demo,
        "/v1/firmware/nodes/999/source",
        &json!({"kind": "asu"}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
