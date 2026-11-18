//! The site file routes over the demo: an export without and with its secrets, imported back
//! (unchanged, then over later edits), onto a site without the file's nodes, with secrets
//! missing and given on accept; files and passphrases refused, stale previews.

use axum::http::{Method, StatusCode, header};
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

fn wlan(ssid: &str) -> Value {
    json!({
        "id": "home", "ssid": ssid, "enabled": true, "network": "lan",
        "security": "wpa2_wpa3", "passphrase": null, "bands": ["2g", "5g"],
        "nodes": { "scope": "all" }, "hidden": false, "isolation": false,
    })
}

/// Saves the latest revision with the WLAN `ssid` and `passphrase` (if given), and the
/// gateway's hostname `gw` when the demo has nodes.
async fn save(demo: &Demo, ssid: &str, passphrase: Option<&str>) -> Value {
    let (_, site) = send(demo, Method::GET, "/v1/site", None).await;
    let mut config = site["config"].clone();
    config["wlans"] = json!([wlan(ssid)]);
    config["wlans"][0]["passphrase"] = site["config"]["wlans"][0]["passphrase"].clone();
    config["site"]["country"] = json!("US");
    if let Some(&gateway) = demo.ids.first() {
        config["nodes"] = json!([{
            "node": gateway, "hostname": "gw", "managed": [], "radios": [], "ports": [], "raw": [],
        }]);
    }
    let secrets = passphrase.map_or(json!({}), |p| json!({ "wlans/home/passphrase": p }));
    let body = json!({ "base": site["revision"]["id"], "config": config, "secrets": secrets });
    let (status, saved) = send(demo, Method::PUT, "/v1/site", Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    saved
}

async fn reveal(demo: &Demo, path: &str) -> String {
    let body = json!({ "path": path });
    let (status, value) = send(demo, Method::POST, "/v1/site/secrets/reveal", Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{value}");
    value["value"].as_str().unwrap().to_owned()
}

async fn preview(demo: &Demo, file: &Value, passphrase: Option<&str>) -> (StatusCode, Value) {
    let mut body = json!({ "file": file });
    if let Some(passphrase) = passphrase {
        body["passphrase"] = json!(passphrase);
    }
    send(demo, Method::POST, "/v1/site/import/preview", Some(&body)).await
}

async fn accept(demo: &Demo, body: &Value) -> (StatusCode, Value) {
    send(demo, Method::POST, "/v1/site/import/accept", Some(body)).await
}

fn sources(preview: &Value) -> Vec<(String, String)> {
    preview["secrets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            (
                s["path"].as_str().unwrap().to_owned(),
                s["source"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

#[tokio::test]
async fn an_export_imports_back_and_over_later_edits() {
    let demo = Demo::start().await;
    let saved = save(&demo, "Home", Some("correct horse")).await;
    let gateway = demo.ids[0];

    // Without secrets: a download anyone signed in may take; not without a session.
    let res = demo
        .send(request(
            Method::GET,
            "/v1/site/export",
            None,
            Some(&demo.cookie),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    let disposition = res.headers()[header::CONTENT_DISPOSITION]
        .to_str()
        .unwrap()
        .to_owned();
    let id = saved["revision"]["id"].as_i64().unwrap();
    assert_eq!(
        disposition,
        format!("attachment; filename=\"uniwrt-site-r{id}.json\"")
    );
    let plain = json(res).await;
    assert_eq!(plain["format"], "uniwrt-site");
    assert_eq!(plain["revision"], id);
    assert_eq!(plain["secrets"], Value::Null);
    assert_eq!(plain["nodes"][0]["node"], json!(gateway));
    assert_eq!(plain["config"]["nodes"][0]["hostname"], "gw");
    let res = demo
        .send(request(Method::GET, "/v1/site/export", None, None))
        .await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    // With them: under a passphrase long enough, audited.
    let body = json!({ "passphrase": "too short" });
    let (status, _) = send(&demo, Method::POST, "/v1/site/export", Some(&body)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let body = json!({ "passphrase": "a long passphrase" });
    let (status, secret) = send(&demo, Method::POST, "/v1/site/export", Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{secret}");
    let armored = secret["secrets"].as_str().unwrap();
    assert!(
        armored.starts_with("-----BEGIN AGE ENCRYPTED FILE-----\n"),
        "{armored}"
    );
    let audit = demo.store.audit_log(10).await.unwrap();
    assert!(audit.iter().any(|entry| {
        entry.action == AuditAction::RevealSecret
            && entry
                .detail
                .as_deref()
                .is_some_and(|d| d.contains("with its 1 secrets"))
    }));

    // Imported back at once, either changes nothing.
    let (status, back) = preview(&demo, &plain, None).await;
    assert_eq!(status, StatusCode::OK, "{back}");
    assert_eq!(back["changes"], json!([]));
    assert_eq!(
        sources(&back),
        [("wlans/home/passphrase".to_owned(), "kept".to_owned())]
    );
    assert_eq!(back["nodes"][0]["node"], json!(gateway));
    let (status, back) = preview(&demo, &secret, Some("a long passphrase")).await;
    assert_eq!(status, StatusCode::OK, "{back}");
    assert_eq!(back["changes"], json!([]));
    assert_eq!(back["file"]["opened"], true);
    assert_eq!(sources(&back)[0].1, "file");
    let (status, wrong) = preview(&demo, &secret, Some("the wrong passphrase")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{wrong}");

    // After edits, the file brings back its SSID and passphrase as a revision of kind `file`.
    save(&demo, "Renamed", Some("another horse")).await;
    let (_, back) = preview(&demo, &secret, Some("a long passphrase")).await;
    assert_ne!(back["changes"], json!([]));
    let (status, site) = accept(&demo, &json!({ "id": back["id"] })).await;
    assert_eq!(status, StatusCode::OK, "{site}");
    assert_eq!(site["revision"]["kind"], "file");
    assert_eq!(
        site["revision"]["message"],
        format!(
            "Imported from a file: revision {id} of uniwrt {}",
            plain["uniwrt"].as_str().unwrap()
        )
    );
    assert_eq!(site["config"]["wlans"][0]["ssid"], "Home");
    assert_eq!(
        reveal(&demo, "wlans/home/passphrase").await,
        "correct horse"
    );
    // Accepted once only.
    let (status, _) = accept(&demo, &json!({ "id": back["id"] })).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn another_site_drops_unknown_nodes_and_asks_for_missing_secrets() {
    let source = Demo::start().await;
    save(&source, "Home", Some("correct horse")).await;
    let (_, mut file) = send(&source, Method::GET, "/v1/site/export", None).await;
    // Another controller, whose nodes have other MACs: the gateway's overrides go; the
    // passphrase is missing.
    file["nodes"][0]["mac"] = json!("02:aa:bb:cc:dd:ee");
    let demo = Demo::start_with(false).await;
    let (status, back) = preview(&demo, &file, None).await;
    assert_eq!(status, StatusCode::OK, "{back}");
    assert_eq!(back["nodes"][0]["node"], Value::Null);
    assert_eq!(back["nodes"][0]["mac"], file["nodes"][0]["mac"]);
    assert_eq!(
        sources(&back),
        [("wlans/home/passphrase".to_owned(), "missing".to_owned())]
    );
    let errors: Vec<&Value> = back["issues"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["severity"] == "error")
        .collect();
    assert_eq!(errors[0]["path"], "wlans/home/passphrase", "{errors:?}");

    // Refused while it lacks it, then saved with a value given; still held after a refusal.
    let (status, refused) = accept(&demo, &json!({ "id": back["id"] })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    let short = json!({ "id": back["id"], "secrets": { "wlans/home/passphrase": "short" } });
    let (status, refused) = accept(&demo, &short).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    let given = json!({
        "id": back["id"], "message": "Moved here",
        "secrets": { "wlans/home/passphrase": "a new horse" },
    });
    let (status, site) = accept(&demo, &given).await;
    assert_eq!(status, StatusCode::OK, "{site}");
    assert_eq!(site["revision"]["message"], "Moved here");
    assert_eq!(site["config"]["nodes"], json!([]));
    assert_eq!(reveal(&demo, "wlans/home/passphrase").await, "a new horse");
}

#[tokio::test]
async fn bad_files_and_stale_previews_are_refused() {
    let demo = Demo::start_with(false).await;
    let (_, file) = send(&demo, Method::GET, "/v1/site/export", None).await;
    for (bad, detail) in [
        (json!({ "schema": 11 }), "This isn't a uniwrt site export"),
        (
            json!({ "format": "uniwrt-site", "version": 2 }),
            "This site export is format version 2; this uniwrt reads version 1",
        ),
    ] {
        let (status, body) = preview(&demo, &bad, None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["detail"], detail);
    }
    let mut newer = file.clone();
    newer["config"]["schema"] = json!(99);
    let (status, _) = preview(&demo, &newer, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // A save after the preview makes it stale.
    let mut renamed = file.clone();
    renamed["config"]["site"]["name"] = json!("Cabin");
    let (_, back) = preview(&demo, &renamed, None).await;
    save(&demo, "Home", Some("correct horse")).await;
    let (status, body) = accept(&demo, &json!({ "id": back["id"] })).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}
