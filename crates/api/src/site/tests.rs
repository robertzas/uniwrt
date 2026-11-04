//! The site and revisions routes over the demo: the defaults of a new site, saves with secrets
//! sealed in the vault and never sent back, conflicts and invalid documents refused, the
//! history paged and compared, restores, and revealing a secret.

use axum::http::{Method, StatusCode};
use serde_json::{Value, json};
use uniwrt_model::user::AuditAction;
use uniwrt_store::AuditRepo as _;

use crate::testing::{Demo, json, request};

async fn send(demo: &Demo, method: Method, uri: &str, body: Option<&Value>) -> (StatusCode, Value) {
    let response = demo
        .send(request(method, uri, body, Some(&demo.cookie)))
        .await;
    let status = response.status();
    (status, json(response).await)
}

async fn put(demo: &Demo, body: &Value) -> (StatusCode, Value) {
    send(demo, Method::PUT, "/v1/site", Some(body)).await
}

fn wlan() -> Value {
    json!({
        "id": "home", "ssid": "Home", "enabled": true, "network": "lan",
        "security": "wpa2_wpa3", "passphrase": null, "bands": ["2g", "5g"],
        "nodes": { "scope": "all" }, "hidden": false, "isolation": false,
    })
}

#[tokio::test]
async fn a_new_site_starts_from_the_defaults() {
    let demo = Demo::start_with(false).await;
    let (status, site) = send(&demo, Method::GET, "/v1/site", None).await;
    assert_eq!(status, StatusCode::OK, "{site}");
    assert_eq!(site["revision"]["id"], 1);
    assert_eq!(site["revision"]["kind"], "defaults");
    assert_eq!(site["revision"]["author"], Value::Null);
    assert_eq!(
        site["config"]["networks"][0]["ipv4"]["address"],
        "10.0.0.1/24"
    );
    let ula = site["config"]["addressing"]["ula"].as_str().unwrap();
    assert!(ula.starts_with("fd") && ula.ends_with("::/48"), "{ula}");
    assert_eq!(
        site["secrets"],
        json!([{ "path": "mesh/key", "set": false }])
    );
    assert_eq!(site["issues"], json!([]));
}

#[tokio::test]
async fn saves_seal_secrets_and_refuse_stale_or_invalid_documents() {
    let demo = Demo::start_with(false).await;
    let (_, site) = send(&demo, Method::GET, "/v1/site", None).await;
    let mut config = site["config"].clone();
    config["wlans"] = json!([wlan()]);
    config["site"]["country"] = json!("US");

    // A passphrase too short, a slot that does not exist, a WLAN without a passphrase.
    let save = |config: &Value, secrets: Value| json!({ "base": 1, "message": " Wi-Fi ", "config": config, "secrets": secrets });
    let (status, body) = put(
        &demo,
        &save(&config, json!({ "wlans/home/passphrase": "short" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["issues"][0]["path"], "wlans/home/passphrase");
    let (status, _) = put(
        &demo,
        &save(&config, json!({ "wlans/nope/passphrase": "correct horse" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, body) = put(&demo, &save(&config, json!({}))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["detail"], "The configuration has an error");
    assert_eq!(body["issues"][0]["path"], "wlans/home/passphrase");

    let secrets = json!({ "wlans/home/passphrase": "correct horse" });
    let (status, saved) = put(&demo, &save(&config, secrets.clone())).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["revision"]["id"], 2);
    assert_eq!(saved["revision"]["message"], "Wi-Fi");
    assert_eq!(saved["revision"]["author"], "admin");
    assert_eq!(saved["revision"]["changes"], 2);
    let reference = saved["config"]["wlans"][0]["passphrase"].as_str().unwrap();
    assert!(reference.starts_with("config/"), "{reference}");
    assert!(!saved.to_string().contains("correct horse"));
    assert_eq!(
        saved["secrets"][0],
        json!({ "path": "wlans/home/passphrase", "set": true })
    );

    // From revision 1 again: stale.
    let (status, body) = put(&demo, &save(&config, secrets)).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");

    // The same document again saves nothing.
    let unchanged = json!({ "base": 2, "config": saved["config"] });
    let (status, body) = put(&demo, &unchanged).await;
    assert_eq!(
        (status, &body["revision"]["id"]),
        (StatusCode::OK, &json!(2))
    );

    // An error elsewhere is reported by path.
    let mut bad = saved["config"].clone();
    bad["networks"][0]["vlan"] = json!(5000);
    let (status, body) = put(&demo, &json!({ "base": 2, "config": bad })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["issues"][0]["path"], "networks/lan/vlan");

    // Revealing is for admins, and audited.
    let (status, body) = send(
        &demo,
        Method::POST,
        "/v1/site/secrets/reveal",
        Some(&json!({ "path": "wlans/home/passphrase" })),
    )
    .await;
    assert_eq!(
        (status, body),
        (StatusCode::OK, json!({ "value": "correct horse" }))
    );
    let (status, _) = send(
        &demo,
        Method::POST,
        "/v1/site/secrets/reveal",
        Some(&json!({ "revision": 1, "path": "wlans/home/passphrase" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let audit = demo.store.audit_log(10).await.unwrap();
    let actions: Vec<_> = audit.iter().map(|entry| entry.action).collect();
    assert!(actions.contains(&AuditAction::Site));
    assert!(actions.contains(&AuditAction::RevealSecret));
}

#[tokio::test]
async fn the_history_pages_compares_and_restores() {
    let demo = Demo::start_with(false).await;
    let (_, site) = send(&demo, Method::GET, "/v1/site", None).await;
    let mut config = site["config"].clone();
    for (base, name) in [(1, "Cabin"), (2, "Lake")] {
        config["site"]["name"] = json!(name);
        let save = json!({ "base": base, "config": config });
        let (status, body) = put(&demo, &save).await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    let (_, page) = send(&demo, Method::GET, "/v1/revisions?limit=2", None).await;
    let ids: Vec<_> = page["revisions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].clone())
        .collect();
    assert_eq!(
        (ids, &page["more"]),
        (vec![json!(3), json!(2)], &json!(true))
    );
    let (_, page) = send(&demo, Method::GET, "/v1/revisions?before=2", None).await;
    assert_eq!(page["revisions"][0]["id"], 1);
    assert_eq!(page["more"], false);

    let (status, two) = send(&demo, Method::GET, "/v1/revisions/2", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(two["config"]["site"]["name"], "Cabin");
    assert_eq!(two["diff"][0]["path"], json!(["site", "name"]));
    assert_eq!(two["diff"][0]["before"], "Home");
    let (_, diff) = send(&demo, Method::GET, "/v1/revisions/3/diff?against=1", None).await;
    assert_eq!(diff["from"], 1);
    assert_eq!(diff["changes"][0]["after"], "Lake");
    let (_, diff) = send(&demo, Method::GET, "/v1/revisions/1/diff", None).await;
    assert_eq!(
        (&diff["from"], &diff["changes"]),
        (&Value::Null, &json!([]))
    );
    let (status, _) = send(&demo, Method::GET, "/v1/revisions/9", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, restored) = send(
        &demo,
        Method::POST,
        "/v1/revisions/1/restore",
        Some(&json!({ "base": 3 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{restored}");
    assert_eq!(restored["revision"]["id"], 4);
    assert_eq!(restored["revision"]["kind"], "restore");
    assert_eq!(restored["revision"]["restored_from"], 1);
    assert_eq!(restored["revision"]["message"], "Restored revision 1");
    assert_eq!(restored["config"]["site"]["name"], "Home");
    // Restoring what is already the latest saves nothing.
    let (_, again) = send(
        &demo,
        Method::POST,
        "/v1/revisions/1/restore",
        Some(&json!({ "base": 4 })),
    )
    .await;
    assert_eq!(again["revision"]["id"], 4);
}
