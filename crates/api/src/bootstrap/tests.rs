//! The first-run routes over demo mode, whose dial reaches only the simulated nodes.

use axum::http::{Method, StatusCode};
use serde_json::json;
use uniwrt_model::user::AuditAction;
use uniwrt_store::{AuditRepo as _, VaultRepo as _};
use zeroize::Zeroizing;

use crate::testing::{Demo, json, request};

#[tokio::test]
async fn the_gateway_search_looks_at_openwrt_s_default_and_finds_no_simulated_router_there() {
    let demo = Demo::start().await;
    let res = demo.send(demo.get("/v1/bootstrap/gateway")).await;
    assert_eq!(res.status(), StatusCode::OK);
    let found = json(res).await;
    let candidates = found["candidates"].as_array().unwrap();
    assert_eq!(candidates.last().unwrap(), "192.168.1.1");
    assert!(candidates.len() <= 2, "{found}");
    assert_eq!(found["gateways"], json!([]));

    let res = demo
        .send(request(Method::GET, "/v1/bootstrap/gateway", None, None))
        .await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    let offline = Demo::start_with(false).await;
    let res = offline.send(offline.get("/v1/bootstrap/gateway")).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn an_admin_sees_a_kept_root_password_and_it_is_audited() {
    let demo = Demo::start().await;
    let id = demo.ids[0];
    let uri = format!("/v1/nodes/{id}/root-password/reveal");
    let reveal = |uri: String| demo.send(request(Method::POST, &uri, None, Some(&demo.cookie)));

    let res = reveal(uri.clone()).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert!(
        json(res).await["detail"]
            .as_str()
            .unwrap()
            .contains("keeps no root password")
    );

    demo.api
        .vault()
        .unwrap()
        .put_secret(
            &format!("node/{id}/root_password"),
            Zeroizing::new(b"abcd-efgh-jkmn-pqrs-tuvw-xyz2".to_vec()),
        )
        .await
        .unwrap();
    let res = reveal(uri).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        json(res).await,
        json!({"value": "abcd-efgh-jkmn-pqrs-tuvw-xyz2"})
    );
    let audit = demo.store.audit_log(10).await.unwrap();
    assert!(audit.iter().any(|entry| {
        entry.action == AuditAction::RevealSecret
            && entry
                .detail
                .as_deref()
                .is_some_and(|detail| detail.starts_with("viewed the root password of"))
    }));

    let res = reveal("/v1/nodes/999/root-password/reveal".to_owned()).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let res = reveal("/v1/nodes/x/root-password/reveal".to_owned()).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}
