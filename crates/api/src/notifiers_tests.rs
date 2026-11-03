//! The notifiers routes over the demo: notifiers created, listed without their secrets, edited
//! with secrets kept, replaced or dropped as their kind needs, refused when invalid, deleted
//! with their secrets (all in the vault); the settings; tests through a webhook on loopback.

use axum::http::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpListener,
};
use uniwrt_core::NotifierId;
use uniwrt_notify::Secrets;
use uniwrt_store::VaultRepo as _;

use crate::testing::{Demo, json, request};

async fn send(demo: &Demo, method: Method, uri: &str, body: Option<&Value>) -> (StatusCode, Value) {
    let response = demo
        .send(request(method, uri, body, Some(&demo.cookie)))
        .await;
    let status = response.status();
    let body = if status == StatusCode::NO_CONTENT {
        Value::Null
    } else {
        json(response).await
    };
    (status, body)
}

fn ntfy(secrets: &Value) -> Value {
    json!({
        "name": " Phone ",
        "enabled": true,
        "spec": { "kind": "ntfy", "server": "https://ntfy.sh/", "topic": "home" },
        "filter": { "min": "warning", "remind_s": 14400 },
        "secrets": secrets,
    })
}

#[tokio::test]
async fn ntfy_keeps_the_secrets_its_kind_uses() {
    let demo = Demo::start_with(false).await;
    let (status, created) = send(
        &demo,
        Method::POST,
        "/v1/notifiers",
        Some(&ntfy(&json!({ "token": "tk_secret" }))),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["name"], "Phone");
    assert_eq!(created["spec"]["server"], "https://ntfy.sh");
    assert_eq!(created["secrets"]["token"], true);
    let id = created["id"].as_i64().unwrap();

    let (status, list) = send(&demo, Method::GET, "/v1/notifiers", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!list.to_string().contains("tk_secret"));
    assert_eq!(list["notifiers"][0]["id"], id);
    assert_eq!(list["notifiers"][0]["status"]["last_ok_at"], Value::Null);
    assert_eq!(list["settings"], json!({ "link_url": null }));
    assert!(
        list["placeholders"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == "title")
    );

    // Secrets left out are kept.
    let uri = format!("/v1/notifiers/{id}");
    let (status, kept) = send(&demo, Method::PUT, &uri, Some(&ntfy(&json!({})))).await;
    assert_eq!(status, StatusCode::OK, "{kept}");
    assert_eq!(kept["secrets"]["token"], true);

    // A username needs its password; then the token, unused, goes.
    let mut with_user = ntfy(&json!({}));
    with_user["spec"]["username"] = json!("me");
    let (status, refused) = send(&demo, Method::PUT, &uri, Some(&with_user)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(refused["detail"], "A username needs its password");
    with_user["secrets"] = json!({ "password": "pw" });
    let (status, changed) = send(&demo, Method::PUT, &uri, Some(&with_user)).await;
    assert_eq!(status, StatusCode::OK, "{changed}");
    assert_eq!(changed["secrets"]["token"], false);
    assert_eq!(changed["secrets"]["password"], true);
}

#[tokio::test]
async fn webhooks_keep_their_url_and_headers() {
    let demo = Demo::start_with(false).await;
    // A webhook: its URL shown as its host, its headers by name; a header without a value keeps
    // the stored one.
    let hook = |secrets: Value, template: Option<&str>| {
        json!({
            "name": "Discord",
            "enabled": true,
            "spec": { "kind": "webhook", "format": if template.is_some() { "custom" } else { "discord" },
                      "template": template },
            "filter": { "min": "critical", "resolved": false },
            "secrets": secrets,
        })
    };
    let secrets = json!({
        "url": "https://discord.com/api/webhooks/1/abc",
        "headers": [{ "name": "X-Token", "value": "t" }],
    });
    let (status, webhook) = send(
        &demo,
        Method::POST,
        "/v1/notifiers",
        Some(&hook(secrets, None)),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{webhook}");
    assert_eq!(webhook["secrets"]["url"], "https://discord.com");
    assert_eq!(webhook["secrets"]["headers"], json!(["X-Token"]));
    let hook_id = NotifierId::new(webhook["id"].as_i64().unwrap());
    let hook_uri = format!("/v1/notifiers/{hook_id}");
    let keep = json!({ "headers": [{ "name": "X-Token" }] });
    let (status, _) = send(&demo, Method::PUT, &hook_uri, Some(&hook(keep, None))).await;
    assert_eq!(status, StatusCode::OK);
    let (_, vault) = demo.api.notify().unwrap();
    let sealed = vault
        .secret(&Secrets::vault_name(hook_id))
        .await
        .unwrap()
        .unwrap();
    let stored = Secrets::from_json(&sealed).unwrap();
    assert_eq!(stored.headers[0].value, "t");
    assert_eq!(
        stored.url.as_deref(),
        Some("https://discord.com/api/webhooks/1/abc")
    );

    // Templates are checked; bad ones say why.
    let (status, refused) = send(
        &demo,
        Method::PUT,
        &hook_uri,
        Some(&hook(json!({}), Some(r#"{"text": {{title}}}"#))),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        refused["detail"]
            .as_str()
            .unwrap()
            .starts_with("The template does not make JSON")
    );
    let (status, _) = send(
        &demo,
        Method::PUT,
        &hook_uri,
        Some(&hook(json!({}), Some(r#"{"text": "{{title}}"}"#))),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send(&demo, Method::DELETE, &hook_uri, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(
        vault
            .secret(&Secrets::vault_name(hook_id))
            .await
            .unwrap()
            .is_none()
    );
    let (status, _) = send(&demo, Method::DELETE, &hook_uri, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// A webhook on loopback answering one request with `status`; the request's JSON body.
async fn webhook(status: &'static str) -> (String, tokio::task::JoinHandle<Value>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/hook", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (mut tcp, _) = listener.accept().await.unwrap();
        let mut data = Vec::new();
        let mut chunk = [0; 8192];
        loop {
            let n = tcp.read(&mut chunk).await.unwrap();
            data.extend_from_slice(&chunk[..n]);
            let text = String::from_utf8_lossy(&data);
            if let Some((head, body)) = text.split_once("\r\n\r\n") {
                let length: usize = head
                    .to_lowercase()
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length: ")?.trim().parse().ok())
                    .unwrap_or(0);
                if body.len() >= length {
                    let response = format!(
                        "HTTP/1.1 {status}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                    );
                    tcp.write_all(response.as_bytes()).await.unwrap();
                    return serde_json::from_str(body).unwrap();
                }
            }
        }
    });
    (url, task)
}

#[tokio::test]
async fn settings_and_tests() {
    let demo = Demo::start_with(false).await;
    let uri = "/v1/notifications/settings";
    let (status, refused) = send(
        &demo,
        Method::PUT,
        uri,
        Some(&json!({ "link_url": "uniwrt.lan" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        refused["detail"],
        "The address is not an http:// or https:// URL"
    );
    let (status, settings) = send(
        &demo,
        Method::PUT,
        uri,
        Some(&json!({ "link_url": " https://uniwrt.lan:8443/ " })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(settings["link_url"], "https://uniwrt.lan:8443");

    let generic = |url: Option<&str>| {
        json!({
            "name": "Hook", "enabled": true,
            "spec": { "kind": "webhook", "format": "generic" },
            "filter": { "min": "warning" },
            "secrets": url.map_or_else(|| json!({}), |url| json!({ "url": url })),
        })
    };
    let (url, server) = webhook("204 No Content").await;
    let test = "/v1/notifiers/test";
    let (status, body) = send(&demo, Method::POST, test, Some(&generic(Some(&url)))).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let sent = server.await.unwrap();
    assert_eq!(sent["event"], "test");
    assert_eq!(sent["url"], "https://uniwrt.lan:8443/insights/alerts");

    // A stored notifier's secrets fill in a test that names it.
    let (url, server) = webhook("500 Internal Server Error").await;
    let (_, created) = send(
        &demo,
        Method::POST,
        "/v1/notifiers",
        Some(&generic(Some(&url))),
    )
    .await;
    let mut named = generic(None);
    named["id"] = created["id"].clone();
    let (status, failed) = send(&demo, Method::POST, test, Some(&named)).await;
    server.await.unwrap();
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(
        failed["detail"],
        "The test failed: the server answered HTTP 500"
    );
    let (status, _) = send(&demo, Method::POST, test, Some(&generic(None))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
